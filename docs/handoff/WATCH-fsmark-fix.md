# WATCH-fsmark-fix — handle reopen mount descriptor

- Attempt: attempt-dae3f6115a58b5796cf47acc3429a327dc4193dc2e69bd2489cb60c561fbdcee
- Branch: hp-dae3f6115a58b5796cf47acc3429a327dc4193dc2e69bd2489cb60c561fbdcee
- Date: 2026-10-13
- Status: implementation and local validation evidence; guest acceptance pending VM.

## Changes

After mark negotiation selects fd/filesystem, open the same configured watched path with `O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_NOATIME | O_CLOEXEC`. Retry only EPERM without O_NOATIME. Compare the replacement's procfs mount ID and fstat device against the original descriptor that anchored the mark; fail startup if either differs. The original descriptor drops immediately after verification, leaving one shared descriptor for handle reopening and mount_identity. Shutdown joins the worker and releases this descriptor through the existing lifetime mechanism; Drop uses the same shutdown. fd/mount and richer modes retain their existing O_PATH descriptor and mark negotiation.

For the REOPENED TARGET, EventHandle::reopen now passes exactly `O_PATH | O_NOFOLLOW | O_CLOEXEC`. On EINVAL only, retry exactly `O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_NOATIME | O_CLOEXEC`. The original implementation used O_PATH | O_CLOEXEC, with the same read-only fallback; this change adds explicit NOFOLLOW to the first attempt. O_PATH performs no content open/read and no access-time update; the fallback is nonblocking, never reads content, and retains NOATIME even on EPERM. No retry without target NOATIME is permitted. Metadata continues to come from the original event descriptor, and the reopened descriptor is only used to resolve its procfs path and then dropped.

The [Linux open_by_handle_at manual](https://man7.org/linux/man-pages/man2/open_by_handle_at.2.html) specifies open-style flags, requires CAP_DAC_READ_SEARCH, and says a symlink handle requires O_PATH and is never dereferenced (NOFOLLOW is ignored by this syscall). Without O_PATH a symlink handle yields ELOOP. The supplied Rocky 8.10 probe establishes that the MOUNT descriptor must be a real open on that kernel; it does not establish that the target O_PATH attempt works. No handle syscall or fanotify runtime was executed here. Confirm these exact flags on the guest; EINVAL from the first target attempt causes the bounded second attempt, whose final error is reported if it too fails.

State has fixed storage for four most recently observed distinct reopen errno values, with saturating counts while each is retained; eviction forgets the old count. All reopen failures, including ESTALE, remain visible in detail. ESTALE does not log. The first unexpected reopen failure logs WARN once per watcher, with mount and OS error; subsequent unexpected failures remain counted without repeated warnings. Unknown errors render as numeric errno (zero denotes an error without a raw OS code). Detail includes `unmapped_events=1 (reopen failed: EBADF count=1)` through Watcher::health_detail. The existing daemon appends health_detail already, so no service edit is necessary. It also appends its existing unmapped_events count separately: a duplicate count in component detail is intentional to preserve the out-of-scope service implementation.

The scale test compares the minimum of ten samples of /proc/self/fd, with nine 100 ms intervals, both before and after the load. This avoids comparing a single transient event batch against an idle descriptor count. No CPU/FD performance claim is made here.

## Scope and contracts

Only watch/mod.rs, tidemark-sys/src/fanotify.rs, tests/w5a/privileged.rs and this note change. No dependencies, manifests, shared module declarations, frozen core types, config keys, metric names, rings or daemon service files change. Existing public signatures remain unchanged. The process_with seam is private and production uses the real identify/reopen methods. All unsafe remains in tidemark-sys with SAFETY comments. No branch creation, merge, push, privileged execution or host configuration change.

## Acceptance evidence

- `mount_descriptor_flags_depend_on_mode`: exact filesystem and mount-mode flag selection.
- `reopen_errors_are_bounded_recent_counted_and_warn_once`: four-entry cap, most-recent order, repeated errno counts, ESTALE silence, one unexpected-warning decision.
- `injected_foreign_reopen_maps_or_reports_badfd`: real metadata/path resolution after injected foreign provenance and reopen result; EBADF retains one event under the root with detailed reason; successful reopen produces the directory below the watched root and zero unmapped events.
- Existing `unmapped_event_keeps_totals_and_root_directory`: aggregation scopes preserved on fallback.
- Existing `filesystem_mark_maps_namespace_bind_and_chroot`: guest directory-mapping regression; pending VM.
- Existing `watcher_sixty_second_load_and_ten_minute_fd_stability`: updated sampling; pending VM.

## Guest commands — pending VM

Run only on prepared disposable Rocky 8.10/4.18.0-553 and Rocky 8.8 guests, as root:

```sh
export CARGO_NET_OFFLINE=true
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a privileged::filesystem_mark_maps_namespace_bind_and_chroot -- --exact --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a -- --nocapture --test-threads=1
timeout 1500 cargo test --offline -p tidemark-live --features privileged --test w5a watcher_sixty_second_load_and_ten_minute_fd_stability -- --ignored --nocapture --test-threads=1
```

Repeat the packaged-daemon check in [WATCH-fsmark.md](WATCH-fsmark.md), using files in distinct directories below the watched XFS mount so root fallback cannot masquerade as correct mapping:

```sh
# Guest only; daemon already configured/running with state outside watched XFS.
mkdir -p /mnt/tidemark-install/fsmark-private-dir
systemd-run --unit=tidemark-fsmark-fix-private -p PrivateTmp=yes /bin/sh -c 'dd if=/dev/zero of=/mnt/tidemark-install/fsmark-private-dir/file bs=1M count=8'
sleep 3
tidemark status
tidemark hot --window 5m
journalctl -u tidemarkd --since '-5 minutes' --no-pager
```

The coordinator's existing eight-writer guest harness must also rerun: plain host, PrivateTmp=yes, PrivateMounts=yes, ProtectHome plus ProtectSystem, subdirectory bind, whole-filesystem bind, RootDirectory plus BindPaths, and chroot. Keep its prior writer/unit/byte fixtures; expect each once, correct unit/bytes, and its actual directory rather than the mount root, with zero unmapped events. Namespace/root/bind setup belongs solely to that disposable guest harness. Inspect status/hot/journal with the commands above after all eight writes. On 8.8 confirm mount fallback and its existing visibility warning; the filesystem regression may skip when unsupported. If failures recur, preserve status detail and strace of name_to_handle_at/open_by_handle_at, including mount and target flags and final errno.

## Local validation

Results and remaining limits are appended after the required checks. Logs are in this attempt's output library. All Cargo commands use pinned Rust 1.98.0, offline dependencies via a copied cache in /tmp/fsmark-cargo, and timeout 1500; privileged features are only compiled/linted, never run. Socket readiness guards may defer five known daemon scenarios in this sandbox. Independent review and actual namespace/bind/chroot, normal unmount, atime and FD-soak evidence remain pending VM.
