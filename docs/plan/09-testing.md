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
| Apparent | `du -sxB1 --apparent-size <dir>` | `t_apparent` |
| Entry counts | `find <dir> -xdev -not -type d -printf . \| wc -c` (robust to newlines in names) | `t_files` |
| Directory counts | `find <dir> -xdev -type d -printf . \| wc -c`, minus mount-point directories and excluded paths, which Tidemark skips | `t_dirs` |
| Distinct inodes | `find <dir> -xdev -not -type d -printf '%i\n' \| sort -u \| wc -l` | `t_inodes` |
| LVM | `lvs --units b --nosuffix -o lv_size,data_percent,metadata_percent`, `vgs -o vg_free` (plain text) | LVM collector values |
| Filesystem totals | `df -B1 --output=size,used,avail` | Sampler values |
| Bulk vs walk | Walk-mode snapshot | Bulk-mode snapshot: all `dirs` columns except `projid` must be equal |

Oracle helpers live in `test-support::oracle` and return parsed numbers. Tests skip with a clear message if `du`/`find` are not GNU.

## 4. Property tests (`proptest`, W2-A)

Generate random trees as an abstract model: directories, files with sizes, link sets with random membership, some incomplete, and symlinks. Feed them to the aggregator through the same `DirRecord` interface the scanners use. Assert:

- All invariants in `03` §3.
- A brute-force reference implementation (sets of inode IDs per subtree) agrees on `t_disk`, `t_inodes`, `t_charged` and `t_reclaimable` for every directory.
- Shuffling the arrival order of records gives an identical output.

## 5. Privileged VM suite (`cargo xtask vm-test`)

- **Targets:** Rocky 8.8 (kernel 4.18.0-477.x) and Rocky 8.10 (latest 4.18.0-553.x), both x86_64, plus optionally Rocky 9 for forward compatibility. Packages: `xfsprogs xfsprogs-devel lvm2 bcc-tools postgresql-server lsof`. The VM layout mirrors production: root and `/data` as XFS on LVM (one VG), with a spare disk for loop/thin tests.
- **Mechanism:** `xtask vm-test --host <ssh-host>` rsyncs the workspace, builds in the VM, and runs `sudo env "PATH=$PATH" cargo test --workspace --features privileged -- --test-threads=1` (EL8 sudo's `secure_path` ignores `-E`). VM provisioning (Vagrant/libvirt or a pre-built image) is documented in `xtask/README.md`.
- **Loop XFS helper** (`test-support::xfs::LoopXfs`): `truncate -s <size> img`, `mkfs.xfs -f [opts] img`, `mount -o loop,<opts>` into a temp dir, with unmount and cleanup on `Drop`. Options cover `ftype=0` (to test the `DT_UNKNOWN` path) and `reflink=0/1`.
- **Loop LVM helper** (`test-support::lvm::LoopVg`): loop devices → `pvcreate`/`vgcreate` with a unique test VG name → optional thin pool and thin LV → `mkfs.xfs` → mount. Cleanup removes LVs, VG, PVs and loop devices on `Drop`, and refuses to touch any VG not created by the test.

Mandatory privileged tests:

| Test | Asserts |
|------|---------|
| `bulkstat_blocks_match_statx` | Bulk disk-bytes conversion equals `statx` for 10,000 random inodes (`04` §5.4) |
| `bulk_equals_walk_*` | Identical snapshots on every mandatory fixture on loop XFS |
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

## 6. Fault injection

| Fault | Method | Expected |
|-------|--------|----------|
| Directory removed mid-scan | Background thread deletes subtrees during the scan | Counted as races; snapshot complete; no panic |
| Permission denied | `chmod 000` as non-root test user | Errors recorded; `degraded = true`; exit code 3 |
| Disk full on state dir | Small loop filesystem as `state_dir` | Space guard refuses; mid-write abort deletes `.tmp`; alert raised |
| Crash during snapshot write | `kill -9` at random points | No `.db` without a complete `meta`; `.tmp` cleaned on next start |
| Crash during time-series compaction | `kill -9` | DB opens cleanly; compaction idempotent |
| fanotify overflow | Generate 100,000 close-writes rapidly | `lossy` minute flagged; counter incremented; watcher keeps running |
| Postgres down / slow | Stop the server; `pg_sleep` behind a lock | Back-off; statement timeout respected; `now` shows the degraded state |
| Huge directory | 1,000,000 entries in one directory | Bounded memory per directory record; progress continues |

## 7. Performance and impact (W7-B)

**`cargo xtask gen-fixture --profile reports`** builds a realistic tree on staging:

- Report directories with 10k–60k files each.
- Log-normal file sizes (median 32 KiB).
- 20% of files hard-linked into 2–4 other report directories.
- 10% symlinks.
- Target 5 M and 20 M inodes.

**`cargo xtask bench --root … --modes du,walk,bulk --cache cold,warm --pgbench on,off`** records:

- Wall time, CPU time, peak RSS, and syscall counts per mode.
- Device util and await percentiles.
- With pgbench on: TPS and p99 latency compared with a no-scan baseline.

Gates (NFR-2), set as defaults and confirmed or adjusted in the pilot:

- Cold walk or bulk, with throttle off, at least 3× faster than `du -sx`.
- With the `normal` throttle profile, pgbench p99 latency degradation ≤ 10%.
- Peak RSS within `memory_budget`.

## 8. Soak (W7)

72 h on staging with:

- The daemon at default settings.
- The fixture generator mutating the tree continuously.
- pgbench running.
- Scans every hour.

Pass criteria:

- RSS flat (no growth trend > 5%).
- fd count flat.
- No missed samples > 1%.
- No unexplained errors in the journal.
- Metrics continuous.

## 9. CI

| Job | Trigger | Content |
|-----|---------|---------|
| `ci` | Every push | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` |
| `build-el8` | Every push to main | Build in a `rockylinux:8` container; upload artifacts |
| `vm-test` | Nightly + before each wave gate | Privileged suite on Rocky 8.8 and 8.10 VMs |
| `fuzz` | Nightly | 10 minutes per fuzz target |
| `bench` | Manual / per release | Staging only |

## 10. Pilot validation on production (read-only)

1. `tidemark probe`: review the output with the team.
2. One walk-mode scan of a single large report subtree with the `gentle` profile at a quiet hour. Compare with `du -sxB1` on the same subtree (also run with `nice`). The numbers must match within the fuzziness of a live tree; investigate any gap above 0.5%.
3. Watch Postgres latency and device await during the scan (`iostat -x 5`, pg monitoring). Record the result.
4. Repeat with bulk mode on the full mount.
5. Only then enable the daemon (see `11-rollout-runbook.md`).
