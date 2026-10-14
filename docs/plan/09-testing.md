# 09 — Testing and validation strategy

The two failure modes that matter most are **wrong numbers** (people delete the wrong thing or panic over nothing) and **hurting production** (scans that stall Postgres, a watcher that sees nothing, a deploy that leaves a host half-configured). The test plan is built around both.

## 1. Test layers

| Layer | Runs where | Privileges | What it proves |
|-------|-----------|------------|----------------|
| Unit | Any dev box / agent sandbox / CI | none | Parsers, math, aggregation, schema, rendering |
| Property | Same | none | Aggregation invariants (`03` §3) on random trees |
| Golden | Same | none | Worked examples and CLI output snapshots (`insta`) |
| Fixture integration | Same | none | Walk-mode scan of real temp trees equals `du`/`find` oracles |
| Privileged integration | **Rocky 8 VM** (8.8 and 8.10) | root, loop devices, xfsprogs, lvm2 | Bulk mode, fanotify, procscan, LVM collector (incl. thin pools), staged deploy + reboot, daemon end-to-end |
| Fault injection | VM | root | Races, permission errors, crashes mid-write, disk full, kill -9 during snapshot write and during `deploy stage` |
| Fuzz | CI (nightly) | none | mountinfo, fdinfo, cgroup, `/sys/block/*/stat`, `lvs`/`vgs` JSON, config parsing |
| Performance | **Staging hardware** (same RAID class as prod) | root | Speed and impact targets (NFR-2) |
| Soak | Staging | root | 72 h daemon stability (NFR-4) |
| Pilot | Production, read-only | root | Real-world correctness and impact, under change control |

`cargo test` runs everything that needs no privileges. Privileged tests are compiled **only** with the `privileged` cargo feature (every member crate declares it). That is the guard; they are not `#[ignore]`d. Long scale tests use `#[ignore = "scale"]`. `cargo xtask vm-test` runs the privileged suite, plus `--suite install|fault` for the script-based suites.

Tests must measure allocated sizes (`st_blocks`), directory sizes and `du` figures rather than assert literals dependent on the filesystem. Overlayfs in a `rockylinux:8` container exposed these assumptions. Assert `03` §6 exactly only in the in-memory reference test and privileged loop-XFS test; ordinary disk fixtures use measured metadata/oracles.

CLI surface tests assert commands, flags, help, parsing and exit-code mapping only; never assert that a command remains unimplemented.

## 2. Fixture DSL (`test-support`, W0-C)

Trees are declared in TOML and materialised into a `tempfile::TempDir`:

```toml
[[entry]]
path = "a/x"
kind = "file"
size = 4096                 # bytes actually written (allocated)

[[entry]]
path = "a/big"
kind = "file"
size = "1MiB"

[[entry]]
path = "a/sparse"
kind = "file"
sparse = "1GiB"             # set_len only: apparent 1 GiB, ~0 allocated

[[entry]]
path = "b/y"
kind = "hardlink"
target = "a/x"

[[entry]]
path = "c/s"
kind = "symlink"
target = "../a"

[[entry]]
path = "locked"
kind = "dir"
mode = 0o000                # unreadable (no effect when running as root; tests account for that)

[[entry]]
path = "weird/\\xff\\xfe-name"   # raw bytes via escape: non-UTF-8 names
kind = "file"
size = 10

[[outside]]                 # created OUTSIDE the scan root, to make link sets incomplete
path = "elsewhere/z2"
kind = "hardlink"
target = "b/z"
```

```rust
pub struct Fixture { pub dir: TempDir, pub root: PathBuf, pub outside: PathBuf }
pub fn materialize(spec: &str) -> Fixture;
pub fn random_tree(seed: u64, params: RandomTreeParams) -> Fixture;   // property tests
```

Shared fixtures live in `test-support/fixtures/*.toml`. Mandatory ones:

| Fixture | Covers |
|---------|--------|
| `link_semantics_basic.toml` | The worked example in `03` §6 |
| `wide_dir.toml` | One directory with 120,000 entries (generated) |
| `deep.toml` | Depth 200 (path length near `PATH_MAX`) |
| `symlink_loops.toml` | Symlink cycles, symlinks to ancestors, dangling links |
| `nonutf8.toml` | Invalid UTF-8 names, newlines in names, names with `\` and quotes |
| `hardlink_web.toml` | Sets spanning 2–50 directories, some incomplete |
| `sparse.toml` | Sparse files and empty files |
| `mixed_special.toml` | FIFOs, sockets, and device nodes (if root) |

## 3. Oracles

| Oracle | Command | Compared with |
|--------|---------|---------------|
| Disk usage | `du -sxB1 <dir>` | `t_disk` for every directory in the fixture (GNU du counts each hard-linked inode once per invocation) |
| Apparent | `du -sxB1 --apparent-size <dir>` | `t_apparent` after normalization below |
| Entry counts | `find <dir> -xdev -not -type d -printf . \| wc -c` (robust to newlines in names) | `t_files` |
| Directory counts | `find <dir> -xdev -type d -printf . \| wc -c`, minus mount-point directories and excluded paths, which Tidemark skips | `t_dirs` |
| Distinct inodes | `find <dir> -xdev -not -type d -printf '%i\n' \| sort -u \| wc -l` | `t_inodes` |
| LVM | `lvs --units b --nosuffix -o lv_size,data_percent,metadata_percent`, `vgs -o vg_free` (plain text) | LVM collector values |
| Filesystem totals | `df -B1 --output=size,used,avail` | Sampler values |
| Bulk vs walk | Walk-mode snapshot of a flushed, unchanged fixture | Bulk-mode snapshot: all `dirs` columns except `projid` must be equal |

`t_disk` is compared directly with `du -sxB1` on every version. The normative `t_apparent` includes directory `st_size` (`03` §2–3). In a `rockylinux:8` container, `du (GNU coreutils) 8.30` was measured to count directory sizes in apparent mode, so equality holds directly on the target. Coreutils 9.2 and later count only regular files and symbolic links in apparent mode.

For apparent-size comparisons, probe the local `du -sxB1 --apparent-size` with an empty directory and compare its output with the measured directory `st_size` to learn whether it counts directory sizes. When it does not, normalize by adding the measured `st_size` of every directory in the compared subtree (including its root), and of any other non-regular, non-symlink inode with non-zero `st_size`, to the `du` figure before comparing with `t_apparent`. Count distinct inodes and use the same subtree boundaries as the metric. The probe decides; never branch on a version string.

Bulk/walk and bulk/apparent-size oracle tests must flush the destination fixture filesystem with `rustix::fs::syncfs` after all writes/copies and before scanning. The fixture writer flushes once after the whole fixture, rather than syncing each file. Production scans never force writeback. `bulk_apparent_size_follows_writeback` writes a 1 MiB file without syncing, checks equal disk bytes and bulk apparent size no greater than walk, then flushes and compares all rows (with the usual mode-only normalization); it must not require the unflushed size to be zero because writeback can happen at any moment.

Oracle helpers live in `test-support::oracle` and return parsed numbers. Tests skip with a clear message if `du`/`find` are not GNU.

## 4. Property tests (`proptest`, W2-A)

Generate random trees as an abstract model: directories, files with sizes, link sets with random membership, some incomplete, and symlinks. Feed them to the aggregator through the same `DirRecord` interface the scanners use. Assert:

- All invariants in `03` §3.
- A brute-force reference implementation (sets of inode IDs per subtree) agrees on `t_disk`, `t_inodes`, `t_charged` and `t_reclaimable` for every directory.
- Shuffling the arrival order of records gives an identical output.

## 5. Privileged VM suite (`cargo xtask vm-test`)

- **Targets:** Rocky 8.8 (kernel 4.18.0-477.x) and Rocky 8.10 (latest 4.18.0-553.x), both x86_64, plus optionally Rocky 9 for forward compatibility. Packages: `xfsprogs xfsprogs-devel lvm2 bcc-tools lsof`. The VM layout mirrors production: root and `/data` as XFS on LVM (one VG), with a spare disk for loop/thin tests.
- **Mechanism:** `xtask vm-test --host <ssh-host>` rsyncs the workspace, builds in the VM, and runs `sudo env "PATH=$PATH" cargo test --workspace --features privileged -- --test-threads=1` (EL8 sudo's `secure_path` ignores `-E`). VM provisioning (Vagrant/libvirt or a pre-built image) is documented in `xtask/README.md`.
- **Loop XFS helper** (`test-support::xfs::LoopXfs`): `truncate -s <size> img`, `mkfs.xfs -f [opts] img`, `mount -o loop,<opts>` into a temp dir, with unmount and cleanup on `Drop`. Options cover `ftype=0` (to test the `DT_UNKNOWN` path) and `reflink=0/1`.
- **Loop LVM helper** (`test-support::lvm::LoopVg`): loop devices → `pvcreate`/`vgcreate` with a unique test VG name → optional thin pool and thin LV → `mkfs.xfs` → mount. Cleanup removes LVs, VG, PVs and loop devices on `Drop`, and refuses to touch any VG not created by the test.

Mandatory privileged tests:

| Test | Asserts |
|------|---------|
| `bulkstat_blocks_match_statx` | Bulk disk-bytes conversion equals `statx` for 10,000 random inodes (`04` §5.4) |
| `bulk_equals_walk_*` | Identical snapshots on every mandatory fixture on loop XFS after flushing the destination filesystem |
| `lvm_maps_mounts_to_lvs` | Loop VG with a linear LV and a thin LV: each mount maps to the right LV; the thin LV maps to its pool |
| `lvm_thin_pool_usage_tracks_writes` | Write data to the thin LV; `data_percent` rises and matches `lvs`; the `thin_pool_pct` alert fires above a test threshold |
| `lvm_absent_is_na` | With `lvm.enabled = true` on a non-LVM loop filesystem, the section reports n/a and nothing errors |
| `root_scan_stays_on_root` | Scanning `/` on the VM never enters `/data`, `/proc`, `/sys`, `/run` or tmpfs (skipped entries recorded). Totals match `du -sxB1 --exclude=/var/lib/tidemark /` within 0.5%, since `/` changes live (journal); exact equality is asserted on a quiescent subtree such as `/usr` |
| `fanotify_fd_mode_sees_close_write` | A child process writes and closes a file; the event arrives with the correct path, PID and size within 1 s |
| `fanotify_probe_reports_capabilities` | Probe output matches actual `fanotify_init` behaviour |
| `watcher_namespace_guard` | Started inside `unshare -m`, the watcher refuses to run |
| `phantom_detects_deleted_open` | Open, unlink, write 100 MiB, then detection with the correct holder |
| `open_writer_growth` | A growing appender is detected; a read-only holder is not |
| `noatime_no_writes` | Scanning a tree mounted `strictatime` leaves directory atimes unchanged |
| `daemon_end_to_end` | Start `tidemarkd` (systemd-run with the real unit options) → write data → `tidemark now` shows the hot directory and unit within 60 s |
| `deploy_stage_reboot_verify` | Install RPM → `deploy stage` → **reboot the VM** → `deploy verify --wait 5m` exits 0; `firstboot.json` present and healthy |
| `deploy_stage_preflight_blocks` | A bad config, missing mount or full state filesystem makes `deploy stage` exit non-zero with **no** changes (service not enabled, config untouched) |
| `deploy_unstage` | `deploy unstage` stops and disables the service and keeps the state dir; `--purge-state` removes it |

### VM runner parameters and disposable md fixture (G6)

Repeatable `--env NAME=VALUE` accepts only names matching `TIDEMARK_[A-Z0-9_]+`; reject other names before making any connection. Transport the pairs through SSH and sudo next to PATH: `sudo env "PATH=$PATH" NAME=VALUE ... cargo test ...`. Quote values so spaces and shell metacharacters cannot escape the command. `--nocapture` appends `--nocapture` to the test binary arguments after `--`, exposing passing-test diagnostics. Unit-test accepted/rejected names, a value containing a space, quote and semicolon, and argument positions.

`xtask/README.md` must document these commands, run **inside a disposable VM only**, never on a development host. Install `mdadm` as a VM prerequisite. Choose an unused md device (example `/dev/md127`), and retain the returned loop paths for cleanup:

```sh
fixture_dir=$(mktemp -d /var/tmp/tidemark-md.XXXXXX)
truncate -s 1G "$fixture_dir/member0" "$fixture_dir/member1"
member0=$(sudo losetup --find --show "$fixture_dir/member0")
member1=$(sudo losetup --find --show "$fixture_dir/member1")
sudo mdadm --create /dev/md127 --metadata=1.2 --level=raid1 --raid-devices=2 "$member0" "$member1"
sudo mdadm --detail /dev/md127
```

Invoke the suite from the checkout using the VM’s SSH alias:

```sh
cargo xtask vm-test --host rocky88 --suite cargo --env TIDEMARK_W1A_MD_DEVICE=/dev/md127 --nocapture
cargo xtask vm-test --host rocky810 --suite cargo --env TIDEMARK_W1A_MD_DEVICE=/dev/md127 --nocapture
```

Provision the fixture separately in each VM. After testing, in that VM only, stop the array and remove only these created resources:

```sh
sudo mdadm --stop /dev/md127
sudo mdadm --zero-superblock "$member0" "$member1"
sudo losetup --detach "$member0" "$member1"
rm -f "$fixture_dir/member0" "$fixture_dir/member1"
rmdir "$fixture_dir"
```

Record these Wave 1 findings on both VMs in `docs/handoff/W1-gate.md`; preserve the diagnostic output:

| Finding | Command / evidence |
|---------|--------------------|
| Bulkstat v5 at runtime | `cargo xtask vm-test --host <vm> --filter bulkstat_probe_records_kernel_support --nocapture`; record the printed kernel/v1/v5 result (module inspection is insufficient) |
| `bs_blocks` unit | `cargo xtask vm-test --host <vm> --filter bulkstat_blocks_match_statx --nocapture`; record the measured conversion and all 10,000 `disk_bytes == statx.blocks * 512` comparisons |
| Default I/O scheduler | In the VM, `sudo tidemark probe --json` for resolved leaves and `cat /sys/block/<leaf>/queue/scheduler` for each leaf; the bracketed scheduler is active |

## 6. Fault injection

Harness-phase tests print a skip line unless their phase/RPM variables are supplied; explicitly requested tests require the explicit VM marker, never inferred. `cargo xtask vm-test --no-fail-fast` passes the option to Cargo before -- and rejects it for script suites. Systemd-launched test binaries are copied out of home to an executable location with restored labels. SSH is the first ssh on PATH, enabling wrappers. Loop/md fixtures do not survive reboot: external harness restores them before verify. Compile-only, printed skips and socket-denied early returns are not guest acceptance. (13 R4-14.)

Packaging assertions follow 13 R4-5: Unit values have no trailing comments; the existing normative unit remains unchanged. Install neither starts nor enables. Upgrade/reinstall uses guarded `try-restart`: restart a running daemon and leave a stopped daemon stopped, preserving enablement. Erase stops/disables and retains state. The install gate requires effective memory limit 2147483648, notify/root, NoNewPrivileges and exact capability names, and rejects installed-unit parser warnings. Active upgrade must change a nonzero PID and regain API readiness; stopped reinstall stays inactive. Edited config survives, and the changed-default fixture must produce root:root 0600 `.rpmnew` with its marker. Enable and prove active/enabled before erase, then assert inactive/disabled and no wants entry even as a dangling symlink; retained `.rpmsave` matches edited bytes and mode 0600. Require SELinux Enforcing unless an explicit permissive override labels SELinux acceptance untested. Post-reboot verification requires an external writer’s hot directory on restored disposable XFS; any post-mount restart is explicit.

| Fault | Method | Expected |
|-------|--------|----------|
| Directory removed mid-scan | Background thread deletes subtrees during the scan | Counted as races; snapshot complete; no panic |
| Permission denied | `chmod 000` as non-root test user | Errors recorded; `degraded = true`; exit code 3 |
| Disk full on state dir | Small loop filesystem as `state_dir` | Space guard refuses; mid-write abort deletes `.tmp`; alert raised |
| Crash during snapshot write | `kill -9` at random points | No `.db` without a complete `meta`; `.tmp` cleaned on next start |
| Crash during time-series compaction | `kill -9` | DB opens cleanly; compaction idempotent |
| fanotify overflow | Generate 100,000 close-writes rapidly | `lossy` minute flagged; counter incremented; watcher keeps running |
| Huge directory | 1,000,000 entries in one directory | Bounded memory per directory record; progress continues |

## 7. Performance and impact (W7-B)

**`cargo xtask gen-fixture --profile reports`** builds a realistic tree on staging:

- Report directories with 10k–60k files each.
- Log-normal file sizes (median 32 KiB).
- 20% of files hard-linked into 2–4 other report directories.
- 10% symlinks.
- Target 5 M and 20 M inodes.

**`cargo xtask bench --root … --modes du,walk,bulk --cache cold,warm`** records:

- Wall time, CPU time and RSS per mode; syscall counts need external strace profiling (not fabricated by the harness).
- Leaf block devices' utilisation and average-wait series with and without a scan, from `/sys/block/*/stat` using the adaptive gate's samples.
- Gate statistics are currently unavailable in frozen scan JSON; report that limitation explicitly.
- Watcher overhead: fixture-generator throughput in mutate mode with and without the watcher.

Gates (NFR-2), set as defaults and confirmed or adjusted in the pilot:

- Cold walk or bulk, with throttle off, at least 3× faster than `du -sx`.
- With the `normal` throttle profile, the leaf block devices' utilisation and average wait (from `/sys/block/*/stat`, using the adaptive gate's samples) stay within the profile's configured targets, apart from short excursions the controller corrects.
- Peak RSS within `memory_budget`.

## 8. Soak (W7)

72 h on staging with:

- The daemon at default settings.
- The fixture generator in mutate mode, mutating the tree continuously.
- Scans every hour.

Pass criteria:

- RSS flat (no growth trend > 5%).
- fd count flat.
- No missed samples > 1%.
- No unexplained errors in the journal.
- Metrics continuous.

## 9. CI

The CLI manifest contains an empty `[package.metadata.generate-rpm]` table for external metadata. `cargo xtask rpm --native` builds on EL8; the changed-default test package requires `--native --version 0.1.1 --upgrade-fixture`. Native mode installs nothing, defaults offline and rejects foreign hosts/missing generator. `tools/check-rpm.sh` checks package identity, exact assets/modes/owners/config flags, scriptlets and extracted ELF binaries. `tools/el8-build-env.sh` prepares disposable EL8 build environments, pinning 8.8 repositories to the vault. GitHub Actions has a `ci` job and separate Rocky 8.8/8.10 `rpm` matrix, locked fetching, package/install verification, checksums and artifacts. ShellCheck gates at warning severity; informational notes are printed on stderr (superseded by R5-17). Owner decision: push main to GitHub after each green local gate; this task does not push. (13 R4-4.)

The implemented GitHub jobs below are ci and rpm; VM/fuzz/bench rows describe required external gates, not evidence of additional hosted jobs.

| Job | Trigger | Content |
|-----|---------|---------|
| `ci` | Every push | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, required `cargo deny check` |
| `rpm` | Workflow push/PR/tag/dispatch triggers | Rocky 8.8 (vault-pinned) and 8.10 native build, package inspection, install/version/rpm verification, checksums and separate artifacts |
| `vm-test` | Nightly + before each wave gate | Privileged suite on Rocky 8.8 and 8.10 VMs |
| `fuzz` | Nightly | 10 minutes per fuzz target |
| `bench` | Manual / per release | Staging only |

## 10. Pilot validation on production (read-only)

1. `tidemark probe`: review the output with the team.
2. One walk-mode scan of a single large report subtree with the `gentle` profile at a quiet hour. Compare with `du -sxB1` on the same subtree (also run with `nice`). The numbers must match within the fuzziness of a live tree; investigate any gap above 0.5%.
3. Watch Postgres latency and device await during the scan (`iostat -x 5`, existing database monitoring). Record the result.
4. Repeat with bulk mode on the full mount.
5. Only then enable the daemon (see `11-rollout-runbook.md`).

## Landed hardening tooling and remaining gates (13 R5-13–17/24/27)

cargo xtask gen-fixture creates marked deterministic wide/sparse/hard-link/symlink fixtures. Finished metadata accounting follows close/flush and labels the summary before_mutation; du compares observed apparent/allocated totals. cargo xtask bench [--tidemark PATH] measures du/scan wall/CPU/RSS, device utilisation/await, cancellation and watcher throughput impact. GNU time and procfs sampling have distinct limits; absent gate JSON is unavailable. Owner staging hardware is still needed for 5M/20M inode runs and the 72-hour soak.

Fault scripts: 01-state-full.sh, 02-snapshot-crash.sh, 03-fanotify-overflow.sh, 04-compaction-crash.sh, 05-directory-races.sh, 06-huge-directory.sh. cargo xtask vm-test --suite fault requires TIDEMARK_DEPLOY_TEST_VM=1; optional TIDEMARK_FAULT_SLOW=1 enables compaction, TIDEMARK_FAULT_SCALE=1 scale, TIDEMARK_FAULT_KEEP=1 retains passing evidence. Failure evidence is retained. Rocky-only tools, fixed locale and 180-second unmount retry cleanup are required. The old healthy-after-120-seconds and compaction description is superseded by R6-10: three normal bursts and a low-limit pass each perform 100,000 close-writes over at most 20,000 reused identities with exact events and zero overflow/loss. A separate 100,000-distinct-file pass must fit one minute and prove aggregation-cap diagnostics with complete events. A separate stopped-daemon burst forces queue overflow. If the optional queue sysctl is absent use the EL8 default 16,384; reject invalid/over-cap values. After SIGCONT, new-write transport recovery must occur within 120 seconds; healthy retained-history expiry is slow-only, bounded by seventeen minutes. Script 04 needs a second-compaction witness and the same live daemon identity: one kill cycle plus verification is about 183 minutes, default two cycles plus verification about 244 minutes. Store subprocess tests remain faster integrity evidence, not a completed guest cycle.

Stable properties/smokes run each CI pass; nightly fuzz/ has 16 targets (table below), 600 s each, 1 MiB input, 2 GiB RSS, two-second execution guard. Pure fanotify parsing owns no arbitrary fd; mount-id and JSON-line frame seams close the original three gaps. Workflow/fuzz execution was not verified in the source notes; cumulative 24-hour crash-free gate is outstanding. Stable timing guards are 30 seconds; 64 KiB versus 1 MiB repeated seeds use three samples/minimum and <200x only if the small input takes at least 1 ms, not a hard complexity proof.

GitHub cargo fetch --locked precedes builds. cargo-deny 0.20.2 is required by owner decision; BSL-1.0 allowed for xxhash-rust. Local absent-tool skip is not a deny pass. cargo xtask ci uses cargo test --workspace --no-fail-fast. ShellCheck warning severity gates; informational notes are printed uncaptured on stderr. Guest acceptance remains pending for trust with a real second user, filesystem marks under the packaged restrictions and symbolic-link mount-boundary/atime behavior.

| Fuzz target | Seam |
|---|---|
| mountinfo | mount table parser |
| disk_stat | device counters |
| cgroup_unit | process unit attribution |
| fdinfo_flags | open flags |
| stat_starttime | process identity |
| lvs / vgs | LVM JSON reports |
| config | bounded TOML configuration |
| api_request / api_response | frozen JSON types/client response |
| staged / firstboot | deployment records |
| catalog_path | persisted snapshot metadata |
| fanotify_events | pure non-owning event buffer |
| mount_id | fdinfo mount id |
| api_frame | incremental bounded JSON-line framing |

## Release-candidate gate evidence (13 R6-10–12)

Script 04 first verifies the original 1,000 historical rows, then seeds 600 due rows under a disjoint mount key. Require exactly two last-value five-minute witness rows, no raw/extra witness rows, unchanged serialization of every historical row column, and the same live non-zombie daemon PID/start time. Each hourly phase keeps its 3,800-second bound. The slow compaction-crash guest cycle has not completed; a helper-fixture pass does not demonstrate real hourly daemon compaction.

Tracing capture assertions run in exact-test child processes: a scoped subscriber alone cannot isolate tracing's process-wide callsite-interest cache. Positive readiness/progress waits use explicit phase barriers and at least ten-second scheduling bounds; install child kill/reap guards immediately, release and join before assertions. Keep short product Busy/deadline bounds only after controlled entry. `cargo xtask ci` runs workspace tests with --no-fail-fast and the strict-ancestry feature regression, including enforcement independent of debug assertions. Real package/guest execution remains distinct from compiled targets and guarded sandbox returns.

Four independent review rounds, verdicts and every finding's landed fix-note trail are recorded in R6-11. No fifth report exists at writing: fourth-round fixes have not been independently re-reviewed. Missing RC-fix-tests/RC-fix-product notes mean the stale sampler component-count assertion and formatting-guard qualification cannot be assumed closed. Owner hardware must still supply 72-hour soak and 5M/20M inode runs; quiet-host fd/filesystem CPU needs repeated measurement (R6-12).
