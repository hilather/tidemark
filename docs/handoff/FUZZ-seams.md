# Handoff — FUZZ-seams: safe seams for the last three parsers

## Needs from coordinator

- Exact `proptest` parity is blocked by the no-dependency/no-main-manifest-change scope: sys, live and daemon have no proptest dev-dependency. This attempt supplies deterministic generated property-style tests instead. Permit manifest changes in a follow-up if proptest shrinking and arbitrary strategy generation are required.
- Build/run the three new nightly targets on GitHub; no local fuzz build was attempted. Run the privileged watcher/real-kernel and unrestricted listener gates in disposable guests. Independent verification and the cumulative release fuzz gate remain pending. Dependency audit also requires an equipped runner because cargo-deny is unavailable here.

- **Attempt:** attempt-344e4e378e741d352655b271e9b25ea13cfdc8b432f96f48025bb23487f0d668
- **Branch:** hp-344e4e378e741d352655b271e9b25ea13cfdc8b432f96f48025bb23487f0d668
- **Date:** 2026-10-13
- **Status:** implementation evidence candidate; not independent task/release acceptance

## What was built

| Seam | Signature | Behavior / bounds |
|---|---|---|
| Non-owning fanotify buffer | `tidemark_sys::fanotify::parse_event_buffer(&[u8]) -> EventRecords<'_>`; iterator item `io::Result<EventRecord>` | Lazy records contain event length, version, metadata length, mask, raw fd integer, pid and absolute trailing-information range. Native-endian safe byte parsing; no syscalls, fd ownership or per-record allocation. Successful records consume at least 24 bytes; first error terminates the pure iterator. FID information lengths, 128-byte handles and NUL-terminated names are validated without copying. |
| Hidden fdinfo mount ID | `tidemark_live::fuzzing::parse_mount_id(&str) -> io::Result<u64>` | Hidden re-export of the unchanged watcher parser. Linear scan, first matching `mnt_id:` line, checked u64 parse. |
| Hidden daemon framing | `tidemarkd::api::read_frame(&mut impl Read, &mut FrameDecoder) -> Result<Request, FrameError>` | Incremental state retains incomplete bytes. Reads one newline-terminated request, leaves following bytes unread and returns typed EOF/incomplete/too-large/JSON/I/O refusals. Framing Vec capacity is at most 1 MiB, including the newline in admission. Read errors return immediately. No socket, clock, dispatch or thread in the seam. |

The existing EventIter alone transfers fd numbers into OwnedFd, including closing descriptors on validation errors and abandoned batches as before. Owning FID output copies only the selected validated handle/name. The pure parser's information scan and owning conversion can scan a FID record twice, still linear and bounded. Error construction uses small constant-size io::Error allocations; the success path and iterator state do not allocate records.

The daemon connection calls read_frame with its retained decoder and one-byte slices after the existing per-byte shutdown/absolute-deadline check. Kernel reads remain in 8 KiB batches. Peer credentials, timeout setup, handler admission/cancellation, uncertain scan-start responses and dispatch are unchanged. No changes to daemon tests outside api.rs.

## Malformed input / compatibility

- Fanotify rejects truncated metadata, wrong version, zero/undersized/out-of-buffer event lengths, metadata outside the event, and unaligned event/metadata/information lengths. The old decoder already rejected the size/version violations, but accepted unaligned lengths. Alignment is **four bytes**, matching Linux's FANOTIFY_EVENT_ALIGN, not eight; a 52-byte DFID_NAME regression ensures legitimate records survive. See [Linux v5.1 fanotify_user.c](https://raw.githubusercontent.com/torvalds/linux/v5.1/fs/notify/fanotify/fanotify_user.c), including its padding calculation. Synthetic existing unit samples now have kernel-style information padding. Owning iteration still skips a plausible malformed record length and closes that record's descriptor; the new pure iterator stops at its first error.
- Mount-ID behavior is unchanged, including refusal when the first matching line is invalid even if a later one is valid. UTF-8 lossy conversion in tests/target matches the watcher boundary.
- Daemon wire behavior is unchanged: the actual frozen protocol is JSON lines, **not length-prefixed**. Exactly 1 MiB including newline is accepted; the next byte is refused. Malformed JSON clears the line, emits the existing bad-request response and permits another request. Partial lines retain the same absolute deadline. A too-large frame closes the connection as before.
- The 1 MiB statement bounds the framing buffer, not the entire serde Request/Value object or allocator internals. Deserialization uses the same serde_json recursion limit and bounded input as before; total allocations are proportional to the admitted frame. No global allocator instrumentation or strict total-deserialization-byte claim is made.

## Tests and targets

New targets: `fanotify_events`, `mount_id`, `api_frame`; registered in fuzz/Cargo.toml, the 16-target workflow matrix and fuzz/README.md. Seed corpora mirror test samples: fd notification, FID and DFID_NAME metadata; fdinfo mount 42; status JSON line. Fanotify binary corpus uses native endian on the supported x86_64 CI/guest architecture.

New stable tests: sys fuzz_smoke (5), live fuzz_smoke (3), daemon api.rs frame tests (4): **12 total**. Existing owning fanotify tests and socketpair API regressions remain enabled unchanged except padding synthetic FID fixtures to match the kernel.

Shared fuzz/support/seams.rs generates 260 arbitrary-byte lengths with seed 0x46555a5a, all seed truncations, four XOR mutations per byte, and duplicated seeds. There are 2,368 generated exercise calls across the four generated tests (886 fanotify, 382 real-descriptor preservation, 574 mount-ID, 526 frame), excluding round-trip/boundary/timing cases. It supplies 30-second guards and the FUZZ-timing scaling pattern: 64 KiB versus 1 MiB, three samples/minimum, <200x comparison only when the smaller input takes at least 1 ms. These are coarse regression guards, not hard watchdogs or complexity proofs. Fanotify tests assert monotonic ranges/progress and that a real descriptor remains open after pure decoding. API tests assert framing length/capacity, JSON round-trip, fragmented input, pipelined malformed/valid lines, exact/oversized boundaries immediate Interrupted refusal and 512-level JSON nesting rejection.

## Shared-file edits and dependencies

- crates/tidemark-live/src/lib.rs: only the requested hidden fuzzing module/re-export.
- fuzz/Cargo.toml: path dependencies on existing tidemark-sys and tidemark-live plus three target registrations. Required to call the actual seams. No new external dependency, main workspace manifest, crate manifest or main lockfile change.
- .github/workflows/fuzz.yml: only three matrix names added.
- Existing ownership headers retained; new Rust files start with Owned by: WFUZZ-seams. Unsafe descriptor construction remains exclusively in tidemark-sys with its SAFETY comment. No unsafe code added elsewhere.

## Assumptions

The canonical attempt branch and explicit scope override generic branch/commit conventions. No branches, merges or pushes were made. No questions were asked in this isolated worker. No dependency was added to bypass the unavailable proptest imports; this limitation is explicitly reported above. Hidden public support seams follow the task's visibility option; shared support code is source-included in the existing fuzz/support style.

All Cargo commands use offline mode, the pinned 1.98.0 toolchain and existing cache through PATH, CARGO_HOME=/home/brewerm/.cargo and RUSTUP_HOME=/home/brewerm/.rustup. Long commands use timeout 1500. No privileged test binary was executed; no mount, storage-management, service or host configuration operation was performed. Unix listener skips here are not runtime listener evidence.

## Validation

All commands ran under timeout 1500 with offline Cargo. Logs are retained in the canonical worker-output directory under library/.

| Command | Observed result | Log |
|---|---|---|
| cargo fmt --check | Final exit 0 | fmt-final.log |
| cargo clippy --offline --workspace --all-targets -- -D warnings | Final exit 0 | clippy-final.log |
| cargo clippy --offline --workspace --all-targets --features privileged -- -D warnings | Final exit 0; compilation only | clippy-privileged-final.log |
| cargo test --offline --workspace --no-fail-fast | Initial exit 101: runtime summaries 779 passed / 0 failed / 14 ignored; four rustdoc dependency-resolution failures | tests.log |
| cargo build --offline --workspace --features privileged --tests | Exit 0; no privileged execution | build-privileged.log |
| tools/ci.sh | Final exit 0; 780 passed / 0 failed / 14 ignored across libtest summaries, including all 12 new tests; fmt, Clippy, source policy, workspace tests/docs and privileged compilation passed | ci-final.log |
| cargo test --offline --workspace --doc | Exit 0 on retry | docs-final.log |
| Standalone changed Rust source formatting | rustfmt --check exit 0 | fuzz-fmt.log |
| Static fuzz registrations | All 16 manifest paths, seed samples and matrix names matched | fuzz-static.log |

Counts aggregate printed libtest summaries, including any child-process summaries; successful listener early returns are not real listener evidence. Final source policy reported 146 owner tags and a clean forbidden-symbol scan. cargo-deny is unavailable and was explicitly skipped by existing CI policy; dependency advisory/license audit still needs an equipped runner. No fuzz build/run or privileged runtime test was performed locally.

Initial Clippy rejected manual modulo syntax; final code uses is_multiple_of and four-byte kernel alignment. The first full test run's runtime assertions passed, but rustdoc could not resolve dependencies in CLI, live, scan and daemon. The later complete documentation retry and final CI passed. The first CI stopped with 669 passes, one failure and 14 ignored: out-of-scope w6a1::blocked_mount_transition_log_does_not_hold_state_lock returned Disconnected at tests/w6a1.rs:1684. It passed in the first workspace run, an isolated exact-test retry (one pass), and final CI. No out-of-scope file changed and no root-cause/fix claim is made for either transient validation failure; the coordinator should retain this history when assessing the test harness.

Parser commits were made separately after their focused tests passed: fanotify 6fb2626, mount-ID ccfc235, daemon framing 34c8154. Final alignment/coverage, documentation commits, cleanup and submission receipt are recorded in the attempt report.

## Guest / equipped-runner reruns — pending

On disposable Rocky 8 guests, run the existing sys fanotify capability test and live w5a watcher suite, especially fd-mode close-write, FID create/delete/rename when supported, watcher mount identity and descriptor stability. Use the project's VM harness for privileged operations, not the worker host. Also rerun daemon W6-A3 listener/credential gates on an unrestricted runner, build/run all 16 nightly fuzz targets, and complete the cumulative 24-hour release gate. No new privileged-only tests were needed for pure decoding; existing privileged suites compile in local validation.
