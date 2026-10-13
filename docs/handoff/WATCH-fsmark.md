# WATCH-fsmark — filesystem-wide fd notifications

## Needs from coordinator

Run the privileged watcher suite and scale gate on disposable Rocky 8.10 and 8.8 guests, and confirm the packaged daemon with a private-namespace writer. No privileged test was executed on this workstation. Independent acceptance and guest performance measurements remain pending.

## What changed

The fd-mode fallback now selects one filesystem mark when `caps.mark_filesystem` is true and marking succeeds, otherwise one mount mark. Rich DFID_NAME negotiation remains unchanged. `WatchMode::FdFilesystem` displays as `fd/filesystem`; daemon state and firstboot input use `fd_filesystem`. The fd transport batch cap applies to both fd modes. Closing/joining the worker releases its group and mark; retained mount identity still comes from the watched descriptor, not an event descriptor.

`tidemark-sys::fanotify::EventHandle` reuses aligned, bounded 128-byte handle storage. Identification uses `name_to_handle_at` with `AT_EMPTY_PATH`; reopen uses `open_by_handle_at` with `O_PATH | O_CLOEXEC`, falling back on `EINVAL` to `O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_NOATIME | O_CLOEXEC`. No contents are read. Failed identification invalidates the handle; reopening an unidentified handle is refused. Every new unsafe block has a SAFETY comment and lives in tidemark-sys. The pure event-buffer parser and its signature/behavior are unchanged.

## Mapping rule and limits

For each non-self fd event in filesystem mode, obtain its originating mount id and handle. Equal mount ids use the event descriptor's path. Different ids reopen against the retained watched mount descriptor and resolve that descriptor's path. There is one readlink on the successful host or foreign path. Metadata, birth-time/ctime handling and pid attribution still come from the original event descriptor. Failure to identify/reopen/resolve, `/`, or a path outside the watched tree falls back to the watched mount point; total, unit and command aggregation continue, and directory aggregation/dirty tracking use the root. A missing path readlink also falls back without discarding available metadata.

The saturating `Watcher::unmapped_events()` counter is included in component detail only when nonzero. It is exposed as a method instead of extending WatchStats: an out-of-scope daemon fixture constructs WatchStats directly. There are no new core/wire fields, metric names or configuration keys.

Hard-linked inodes may map to a different name, so directory attribution is best effort. For a watched bind of a subdirectory, filesystem marks see writes outside that subtree too; those whose reopened path is `/` or outside the subtree are counted at its root. The existing literal ` (deleted)` suffix is preserved, with the same parent-directory treatment as before; failed reopen of an unlinked inode instead uses the root fallback. Exclusions are evaluated on the selected host path/root with the existing exclusion logic.

`fd/mount` component detail ends with `writes from other mount namespaces and bind mounts are not seen`; startup logs it once at WARN with the mount. Filesystem mode logs `all mounts of the filesystem are seen` at startup. Both healthy modes remain `ok`.

## Shared files and CLI

No shared module declarations, manifests, dependencies, core types, daemon integration-test files or plan documents changed. Only assigned paths changed.

The CLI has no separate enum-to-mode table requiring this variant. `crates/tidemark-cli/src/render/status.rs:61` prints the daemon mode string directly; component detail already carries the display form. Foreground watch uses WatchMode Display. No CLI change is needed for recognition. A future cosmetic change could turn `fd_filesystem` into `fd/filesystem` in the status mode line, consistently with other modes; no such out-of-scope edit was made. `now` still says `fd mode only` for fd reporting, because create/delete/rename notifications remain unavailable in fd mode; this does not assert mount coverage. Firstboot needs no edit beyond the state mode string already supplied.

## Tests and acceptance evidence

Added unprivileged tests:

- `fanotify::tests::unidentified_handle_is_rejected`: bounded handle lifecycle refusal.
- `watch::fdmode::mapping_tests::provenance_and_bounds`: same/foreign mount ids, successful reopen, failed reopen, failed identification, outside paths, `/`, raw deleted suffix.
- `watch::tests::fd_mark_selection`: supported/successful, supported/failed, unsupported capability and display string; no second mark on success.
- `watch::tests::unmapped_event_keeps_totals_and_root_directory`: injected watcher plus processing seam retains total and all three scopes under root fallback.
- Strengthened existing `single_statx_matches_fstat_and_ctime_fallback` with distinct metadata and path descriptors: failed statx still fstats the original event descriptor.
- Daemon source-module tests `filesystem_watch_mode_state_string`, `mount_mode_detail_reports_blind_spots`, and `filesystem_mode_detail_is_healthy`: state/firstboot string and detail/health for both modes.

Added privileged test `filesystem_mark_maps_namespace_bind_and_chroot`: private namespace, subdirectory bind and chroot writes map into the watched directory, each once; mode, totals, directory rows and zero unmapped count checked. It prints a SKIP line when filesystem marks are unavailable. Bind cleanup precedes watcher stop, and explicit LoopXfs cleanup asserts that the filesystem and loop resources can be released. The existing ten-minute scale test selects the new mode automatically on Rocky 8.10 and prints mode, CPU percentage and microseconds/event; it retains its descriptor-stability and throughput checks. Privileged tests are compiled only here, pending VM.

Final local command results and test counts are recorded below after validation. Logs are retained in the attempt output `library/`.

## Cost and measurements

The coordinator's Rocky 8.10 C probe measured 6.1–7.0 microseconds for handle identification, reopen, readlink and close, versus 2.5–2.8 for readlink alone (10,000 iterations). These are syscall-path probe figures, not a Rust watcher measurement. This implementation adds one identification call to each eligible filesystem-mode event; equal-id events avoid reopening. A single retained handle buffer adds no per-event handle allocation; path resolution keeps the existing path allocation pattern and does not copy the mapped path a second time. Foreign events use their reopened descriptor for the sole path readlink. The coordinator must measure the complete watcher on guests with aggregation and attribution included.

## Proposed plan text (not applied)

07 §3.1: “Fd reporting probes filesystem marks independently of FID reporting. When supported and successful, fd/filesystem uses one filesystem mark and sees writes through all mounts of that filesystem, including bind mounts and private mount namespaces. Otherwise fd/mount uses one mount mark and explicitly warns that writes through other mounts are not seen. Each filesystem-mode event's mount provenance determines whether its path is resolved directly or reopened by handle against the retained watched mount. Unmappable events remain in totals/unit/command scopes and use the watched root in directory scope; component detail reports unmapped_events when nonzero. No configuration key is added.” Update the fd-mode pseudocode accordingly.

ADR-013: “Filesystem marks are available on measured Rocky 8.10 (-553), but not Rocky 8.8 (-477). Prefer them in fd mode and retain mount-mark fallback. Keep the host namespace guard and retained mount-identity verification: filesystem-wide visibility does not by itself make daemon filesystem sampling, scanning or pathname interpretation safe in a private namespace. No service sandbox or capability change is authorized by this watcher change.”

Operator guide: “Inspect watcher mode and component detail. fd/filesystem covers other mount namespaces and bind mounts; directory names are best effort for hard links and watched subdirectory binds. unmapped_events counts events attributed to the watched root after path mapping failed. fd/mount remains operational but misses writes through other mount objects; its detail and startup warning state this limit. Long-lived files without close remain covered by the process scanner and filesystem rates.”

## Guest commands — execute only in disposable VMs

As root inside the project's disposable Rocky guest with XFS, util-linux and Python 3 available:

```sh
# Rocky 8.10: namespace/bind/chroot mapping and existing watcher suite.
cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
# Ten-minute descriptor stability and CPU/us-per-event measurement.
cargo test --offline -p tidemark-live --features privileged --test w5a watcher_sixty_second_load_and_ten_minute_fd_stability -- --ignored --nocapture --test-threads=1
```

With the packaged daemon configured to watch the guest gate's existing XFS mount `/mnt/tidemark-install`, its state outside that mount, and the daemon already running:

```sh
# Rocky 8.10 only; replace this path with the actual watched guest gate mount.
systemd-run --unit=tidemark-fsmark-writer -p PrivateTmp=yes /bin/sh -c 'dd if=/dev/zero of=/mnt/tidemark-install/fsmark-private bs=1M count=8'
sleep 3
tidemark status
tidemark hot --window 5m
journalctl -u tidemarkd --since '-5 minutes' --no-pager
# Expect mode fd_filesystem / detail fd/filesystem, correct watched directory,
# and the startup line: all mounts of the filesystem are seen.
cat /var/lib/tidemark/deploy/firstboot.json
# Inspect the report only after a fresh gate deployment if checking firstboot.
```

Rocky 8.8: run the same w5a suite and confirm the new mapping test prints its unsupported-filesystem-mark SKIP line. With the packaged daemon, `tidemark status` must show state mode `fd_mount`, detail mode `fd/mount`, state `ok` when otherwise healthy, and the exact visibility warning; `journalctl -u tidemarkd` must show one WARN per watcher start with the mount. A PrivateTmp writer is expected to remain invisible in this fallback mode. Do not reinterpret healthy state as whole-filesystem visibility.

## Assumptions and remaining uncertainty

- Explicit attempt scope and commit instructions override generic branch conventions; no branch creation, merge or push occurred.
- The coordinator's kernel measurements are supplied evidence; no fanotify runtime probe or privileged operation was run here.
- Rocky 8 kernels prefer fd reporting because DFID_NAME is unavailable there. The existing richer-mode behavior on other kernels remains as before.
- O_PATH rejection is detected by EINVAL; other errors conservatively count as mapping failure. Read-only fallback never drops NOATIME or NONBLOCK.
- The packaged capability bounding set already contains CAP_DAC_READ_SEARCH; no packaging change was needed.
- Root fallback honors existing configured root exclusions. No path is invented below the watched root to make unmappable directory attribution look precise.
- Seven new unprivileged test declarations and one privileged declaration were added; runtime aggregate counts may include child-process summaries and existing early-return socket tests.

## Final local validation

Pinned Rust 1.98.0 with `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`. Every long command ran under `timeout 1500`. Final source was frozen before this sequential run; only the handoff/report were updated afterward.

| Required command | Exit | Evidence under attempt output library/ |
|---|---:|---|
| `cargo fmt --check` | 0 | final/fmt.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | final/clippy.log |
| `cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings` | 0 | final/clippy-privileged.log |
| `cargo test --offline --workspace --no-fail-fast` | 0 | final/tests.log |
| `cargo build --offline --workspace --features privileged --tests` | 0 | final/build-privileged.log |
| `tools/ci.sh` with offline environment | 0 | final/ci.log |

Final workspace and CI logs each aggregate to **820 passed, 0 failed, 14 ignored** across printed libtest summaries (including child-process summaries). All seven new unprivileged tests and the strengthened metadata fallback regression passed. Ignored tests are existing scale gates. Existing socket/root-dependent early returns mean these totals do not establish real listener/operator scenario execution. No privileged test binary was run.

Final source policy: `148 owner tags; forbidden-symbol scan passed`. CI warns that cargo-deny is not installed and skips the dependency audit under its existing policy; an equipped coordinator runner must perform that audit. No dependencies were added.

Earlier full runs passed their runtime assertions but failed four rustdoc targets with E0463 dependency-resolution errors (`tidemark-cli`, `tidemark-live`, `tidemark-scan`, `tidemarkd`). These occurred while other builds/checks were active; no root-cause claim or out-of-scope repair is made. Both final sequential workspace tests and CI passed documentation checks. Earlier results remain in library/tests.log and library/ci.log; final/ is the authoritative local command evidence for the completed source.

## Definition of Done evidence

- [x] Only assigned paths changed; no shared declarations or manifests edited.
- [x] Final formatting and Clippy pass, including privileged compilation targets.
- [x] Final unprivileged workspace tests and available CI checks pass.
- [x] Privileged tests written and compiled; listed above as pending VM.
- [x] New public items documented; unsafe remains in tidemark-sys with SAFETY comments.
- [x] Handoff records mapping, scope, limits, assumptions, guest commands and test counts.
- [ ] Guest runtime/performance acceptance and independent coordinator verification remain pending.
- [ ] Dependency audit pending a runner with cargo-deny.

Cleanup, clean-checkout confirmation, final candidate commit and submission receipt are recorded in the external attempt report. This handoff is an evidence candidate, not an assertion of independently verified task success.
