**NOT READY** — gate checkout: **2 BLOCKER, 6 SHOULD-FIX, 1 NIT** outstanding (eight new findings below plus unresolved original W1-A #4). The exact blind-review candidate additionally retains original W1-B #1: **2 BLOCKER, 7 SHOULD-FIX, 1 NIT**. This is review evidence, not approval, integration, or authorization to start another wave.

## Scope and method

Reviewed exact candidate `d8f130fe32e97322f1bbedd3781ece1a6023fa79`, base `febc94bf661333de008a1de20f2779102bc6843c`, with whole-tree scope. The supplied gate checkout is `578d7c555830c3d061d89968debd3d59a434d403`. Comparing trees shows only three differences: `crates/tidemark-fs/src/walk/worker.rs`, `crates/tidemark-fs/tests/w1b/main.rs`, and `docs/handoff/W1-B-fix.md`. All other reviewed code and plans are identical. Workspace builds ran on the supplied checkout; the earlier candidate's complete walker sources were separately extracted, compiled against unchanged dependencies, and tested with the later tie regressions. No candidate provenance investigation was performed.

Read AGENTS, the whole Wave 1 plan and its relevant read-first storage/scanner/link/live/architecture requirements, all five original reviews, all W1 task/fix/preparation handoffs, all four Wave 1 SCR files, contract decisions, playbook §§5/7, and downstream Wave 2/5 interfaces. Source inspection, checked-in regressions, independent SQL comparisons, source-copy probes and an ABI mutation test support the conclusions. Only this repository report was changed.

The brief supplies accepted Wave 1 decisions, but their promised transcription into `docs/plan/13-contract-decisions.md` is absent from both reviewed trees. This report distinguishes those accepted review assumptions from stale checked-in plan text; it does not approve further contract changes.

Evidence files named below are under this attempt's output `library/`:
`/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-02e84e5610f554c3995500462c42b3a3007c2580881ef97343376730e197f16e/library/`.

## Original review findings

Statuses here refer to the gate checkout unless the candidate differs. There are **14 RESOLVED, 1 NOT RESOLVED, 0 PARTLY RESOLVED** in that checkout. At the exact candidate, B #1 is also NOT RESOLVED, leaving 13 resolved. Resolution credits the specific original defect, not all acceptance criteria for its task.

| Original finding | Status | Code and regression evidence |
|---|---|---|
| W1-A #1 — nsfs disables discovery | RESOLVED | `fs/src/mount.rs:18,51,95` stores `MountRoot::Object` bytes and preserves valid neighbors. `nsfs_object_root_keeps_table_and_boundaries` verifies nsfs boundary membership and invalid UTF-8 object bytes; `malformed_records_preserve_neighbors` verifies continued parsing. These pass. Partial-table boundary safety is separately G4. Paths beginning `fs/` in this table mean `crates/tidemark-fs/`. |
| W1-A #2 — normalize before realpath | RESOLVED | `crates/tidemark-cli/src/cmd/probe.rs:73` routes original CLI PathBufs into config; `fs/src/probe.rs:259` canonicalizes those and configured PathBufs before RawPath normalization; failed originals retain unknown facts. `tests/w1a/paths.rs` covers symlink/parent traversal into `/proc`, missing intermediates and relative failures through CLI/config routes; all pass in CI. |
| W1-A #3 — source display controls | RESOLVED | `fs/src/mount.rs:127` escapes controls/backslashes for UTF-8 and invalid-byte sources; CLI `tests/w1a/render.rs` parser-to-renderer regression covers newline, ESC and backslash. Passing CI; duplicated implementation is G8. |
| W1-A #4 — captured EL8 mountinfo absent | NOT RESOLVED — SHOULD-FIX | `fs/tests/w1a/data/README.md` still labels supplied inputs synthetic. `captured_el8_inputs_pending_vm` at `mount.rs:78` prints SKIP and returns when captures are absent; this review observed that skip. A passing test is not capture evidence. Owner W1-A/human fixture collection must supply 3–4 genuine EL8 samples with provenance and exercise them. |
| W1-B #1 — top-N cutoff ties | RESOLVED at gate checkout; NOT RESOLVED at exact candidate | `fs/src/walk/worker.rs:565` now admits all size-cutoff ties and maintains a size-only heap. Both new same-directory/cross-directory regressions pass locally. Replaying them against candidate walker sources fails the reverse-created cross-directory test (exit 101); see `candidate-w1b-replay/replay.log` and `current-w1b-ties.log`. Count/byte bounds need separate G3 follow-up. |
| W1-C #1 — v1 truncates nlink | RESOLVED | `crates/tidemark-sys/src/xfs.rs:77,252,299` retains v1 only as a Boolean diagnostic probe; only `convert_v5` constructs BulkInode. `unavailable_v5_never_returns_legacy_metadata` forces ENOTTY/EINVAL/EOPNOTSUPP/ENOSYS, asserts the first/only Unsupported item, zero v1 requests and fused termination. Wide 70,000-link/project-id and EPERM tests pass. |
| W1-C #2 — incomplete ABI offsets | RESOLVED | `crates/tidemark-sys/src/gen/bindings.rs:187,251` checks every one of 110 declared offsets across eight structs plus sizes/alignments. Exact-source tests pass; swapping v5 UID/GID in an external copy fails `upstream_v5_layout` at bs_uid 72 versus 68 (expected exit 101). `xfs-layout-coverage.log`, `xfs-bindings-probe.log`, `xfs-bindings-mutant.log`. |
| W1-D #1 — waiting requests ignore rate changes | RESOLVED | `fs/src/throttle.rs:54,77,86` accounts old-rate elapsed credit under the wait mutex, changes rate, notifies and recomputes debt in a loop. `time_weighted_debt_and_burst`, `concurrent_waiters_follow_rate_decrease`, `bulk_waiter_follows_decrease_and_recovery`, and four-thread throughput pass. Independent raising-rate/zero/stop checks pass; distinct cumulative precision issue is G7. |
| W1-D #2 — non-UTF-8 Name breaks UID | RESOLVED | `fs/src/procfs.rs:143` locates bounded byte Uid line before parsing it. `uid_parser_and_error_policy` includes unrelated Name byte 0xff with valid Uid, missing/negative/overflow fields; passes. |
| W1-D #3 — unapproved FixedGate constructor | RESOLVED | `fs/src/throttle.rs:121` constructor is private; throughput regression moved into the same module. No replacement public constructor. Factory observation contract needs G2. |
| W1-E #1 — retention deletes last complete | RESOLVED | `crates/tidemark-store/src/retention.rs:28,45` sorts per raw-byte root exactly as catalog latest (timestamp then scan-id descending) and protects index zero regardless of policy. `retention_preserves_newest_with_expired_windows_for_each_root` covers multiple roots, zero recent, expired windows and all zeros; existing UTC/DST/ISO-week table passes. |
| W1-E #2 — glob filter unimplemented | RESOLVED | `crates/tidemark-store/src/reader.rs:434` builds literal-separator globs and matches `Candidate::from_bytes`, tracks deepest matching ancestor regions iteratively, streams candidates/instances, then limits results. `glob_boundaries_match_raw_bytes_and_nested_regions` covers invalid bytes versus replacement text, missing/shared/nested/overlapping regions, outside region, invalid pattern, depth conflict and zero limit; passes. |
| W1-E #3 — quadratic sample queries | RESOLVED | `crates/tidemark-store/src/timeseries.rs:493` uses fixed res equality with exact/bounded timestamp seeks. Plan regression verifies all five inner searches for each family. 24,024-row retained-history and overlapping-tier tests pass. Independent extraction of old/new production SQL compared 6,000 query pairs over 100 seeded histories, all three desired tiers, negative timestamps/bucket edges and optional PG name: identical rows. `sample-query-differential.py/.log`. |
| W1-E #4 — no explicit corrupt recovery | RESOLVED for accepted explicit entry point | `crates/tidemark-store/src/catalog.rs:272` creates a private unique recovery directory, renames original DB/WAL/SHM without deletion/replacement, syncs directories, then creates/rebuilds fresh storage. `recover_corrupt_catalog_preserves_database_and_sidecars` checks byte preservation and repeated recovery namespaces; passes. Recovery is callable without first constructing Catalog. Ordinary failed-open preservation conflict is separately G5; historical sample rows remain unavailable in the fresh DB. |
| W1-E #5 — floor rounded down | RESOLVED | `crates/tidemark-store/src/space_guard.rs:160` decodes validated finite 0..100% f64 into an exact rational and uses checked u128 ceiling arithmetic. `space_floor_does_not_round_down_large_sizes` covers 2^62+512 at 100% and fractional 12.5% with a one-byte remainder; passes. |

## New findings

### G1 — BLOCKER: downstream bulk-selection contract still selects an unsupported format

`docs/plan/waves/wave-2-scan-engine.md:172`; `docs/plan/04-scanner.md:240`; `crates/tidemark-sys/src/xfs.rs:140`. Reference `finding:w1-gate-bulk-contract`.

W2-C's frozen ModeFacts exposes `bulkstat_v1`, and scanner instructions permit legacy iteration, while the accepted W1-C change permits only v5 metadata. For a kernel reporting v1=true/v5=false, following that selector permits bulk but the iterator's first item is Unsupported. The scanner also still calls legacy nlink 32-bit, describes v1 resume semantics and proposes filesystem-block units. `docs/plan/13-contract-decisions.md` contains only Wave 0 decisions; Wave 1 still declares MountInfo.root as RawPath and immutable throttle deadlines.

**Change/owner:** coordinator's parallel contract-plan task must land and be reviewed before W2-B/C: explicitly select on v5, synchronize iterator/link-width/byte-unit/cursor guidance, MountRoot, canonicalization order, token debt, ordered bounded samplers, newest-complete retention, explicit recovery and cutoff ties. Do not reinterpret a field named bulkstat_v1 as v5. This finding identifies missing published preparation, not an unauthorized W1 implementation change.

### G2 — BLOCKER: the required gate factory erases the statistics W2-C must persist

`crates/tidemark-fs/src/throttle.rs:344`; `crates/tidemark-core/src/scan.rs:268`; `docs/plan/waves/wave-2-scan-engine.md:193,198`. Reference `finding:w1-gate-stats-handle`.

`gate_from_config` returns `Arc<dyn Gate>`, whose only methods are acquire/current_rate. AdaptiveGate's stats/stop are inherent methods and unavailable through that return value; fixed/noop statistics have no common observation API. The prescribed W2-C flow cannot both use the factory and obtain real SnapshotMeta.gate_stats or explicitly stop sampling before final measurement.

**Change/owner:** W1-D plus contract coordinator must expose an approved factory companion/handle or coordinated Gate extension for statistics and shutdown, defining fixed/noop results too. Add a caller-level regression that constructs through the factory, throttles, stops and reads final statistics. Avoid duplicating profile selection or storing invented zero counters in W2-C.

### G3 — SHOULD-FIX: record-count limits do not establish a usable memory bound for cutoff ties

`crates/tidemark-fs/src/walk/worker.rs:22,77,565`; `crates/tidemark-fs/tests/w1b/main.rs:343`; `crates/tidemark-core/src/config.rs:187`. Reference `finding:w1-gate-walker-byte-budget`.

The tie fix is mathematically bounded, not unbounded: heap <=top_files, direct entries <=1,000,000, queued records <=1,024, workers <=64. But whole DirRecords retain every tied candidate and allocated name. The measured NotableFile layout alone is 56 bytes. Twenty-four queued one-million-candidate directories need 1,344,000,000 bytes before names/capacity/other state, already exceeding the default 1,342,177,280-byte memory budget. Default eight workers plus 1,024 queued full records allow a 57,792,000,000-byte lower bound before names. Eight active million-entry records with 255-byte names exceed 2 GiB even with a tiny output queue. More than one million direct entries causes a fatal Limit, not truncation.

The existing 120k wide-directory scale test passes but sets top_files=0 and threshold=u64::MAX; it cannot validate tie retention memory. This limitation preexists for threshold/provisional candidates and becomes common for equal-sized/empty files with the tie change. Scanner §7 permits walk estimates exceeding budget while NFR-4 expects bounded memory, so this is also a contract decision.

**Change/owner:** coordinator + W1-B/W2-A/C must agree a byte-budget/backpressure strategy, including in-progress records; chunk/spill contracts if necessary, or an explicit bounded refusal policy before allocations. Preserve all required ties. Add a slow-consumer multi-directory equal-size test asserting RSS against the selected budget. A channel item count or cgroup OOM is insufficient evidence of graceful bounded operation.

### G4 — SHOULD-FIX: partial mount parsing can bypass the walker's fail-closed boundary check

`crates/tidemark-fs/src/mount.rs:60`; `crates/tidemark-fs/src/walk/worker.rs:98,375`. Reference `finding:w1-gate-partial-mount-boundaries`.

Input consisting of a valid `/root` root record followed by `2 1 8:0 /sub /root/bind rw - xfs /dev/sda` (missing final super_opts) returns Ok with only the first record. `/root/bind` is recoverable from that line but absent from submounts. The walker refuses unavailable mount discovery only on Err when statx MNT_ID is zero; Ok(partial) bypasses that branch. On a kernel without MNT_ID, a same-device bind at that discarded target passes the remaining device check. Parser behavior was probed; the conditional traversal path was inspected. This is not a claim that the current host kernel emitted malformed mountinfo.

**Change/owner:** W1-A/B jointly preserve independently decodable mount boundaries from rejected records, or expose table completeness and require reliable mount IDs/fail closed for scans using incomplete tables. Discovery may still report valid facts. Add a malformed-nonboundary-field + unavailable-MNT_ID regression. Valid nsfs boundaries are already fixed.

### G5 — SHOULD-FIX: ordinary failed open can remove artifacts before explicit recovery

`crates/tidemark-store/src/timeseries.rs:46`; `crates/tidemark-store/tests/storage.rs:1124`; `docs/handoff/scr-W1-E/SCR-1.md`. Reference `finding:w1-gate-recovery-open-order`.

A temporary state directory containing corrupt bytes in timeseries.db and both sidecars reproduces: `Catalog::open(state,true)` returns NotADatabase, leaves DB bytes, but SQLite removes WAL/SHM; subsequent recover_catalog preserves only DB. The checked-in test recreates sidecars after the failed open. `catalog-open-probe.log` records the result. A separate valid-WAL/header-damage case was successfully recovered by SQLite itself; this review does **not** claim loss of valid committed rows from that case.

The original review asked for nondestructive ordinary opens; accepted SCR-1 instead preserves ordinary-open behavior and suggests invoking recovery after failed open. The explicit recovery implementation obeys that accepted API, but the suggested workflow cannot promise preservation of every original recovery artifact.

**Change/owner:** coordinator + W1-E/W2-C must resolve this conflict and define a preservation-before-mutation recovery workflow or suitable corruption preflight. Test the actual open-failure-to-recovery command sequence without reinstalling sidecars. Preserve raw evidence even if SQLite judges it invalid; do not silently broaden automatic destructive recovery or claim the existing direct-recovery test covers this path.

### G6 — SHOULD-FIX: the documented VM runner does not supply its required md fixture environment

`crates/tidemark-fs/tests/w1a/main.rs:69`; `xtask/src/vm_test.rs:78`. Reference `finding:w1-gate-md-vm-environment`.

The md test requires TIDEMARK_W1A_MD_DEVICE and >=2 disposable-array leaves. The runner transports only PATH through sudo, has no fixture/environment option and documents no md provisioning. A plain host-side export does not carry this value through SSH and sudo; a vanilla full VM run reaches the test's expect failure.

**Change/owner:** W1-A/test infrastructure preparation must supply a disposable VM fixture and an explicit validated way to pass its device into the privileged test process, with runner coverage. No array should be created on the development host. Resolve before claiming either full VM suite green. Also retain passing diagnostic output: the runner lacks --nocapture/--show-output, so the v5 test's eprintln is otherwise hidden on success.

### G7 — SHOULD-FIX: long-lived floating-point token counters eventually stop charging units

`crates/tidemark-fs/src/throttle.rs:51,55,92`. Reference `finding:w1-gate-token-counter-precision`.

issued/earned accumulate forever as f64. At issued=earned=2^53, adding cost 1 rounds back to the same value. An exact-source copy with an appended test seeded this state at rate 1/s; 1,000 actual acquire(1) calls completed in 262.689 microseconds with no counter advance. This violates conservation. It is an accelerated numeric-state probe, not an endurance run. Reaching the threshold at the legal u32::MAX rate takes roughly 24.27 days; default-rate scan lifetimes make it far less practical. No cumulative cap exists.

**Change/owner:** W1-D should use exact integer/fixed-point reservations with checked overflow, or coordinated epochs/rebasing that also preserves outstanding waiter targets. Resetting shared counters alone is wrong. Add near-2^53 and outstanding-waiter regression cases. Ordinary-rate concurrent rate-change behavior is sound in the inspected tests; this is distinct from resolved D #1.

### G8 — NIT: byte display escaping has two identical implementations

`crates/tidemark-core/src/path.rs:253`; `crates/tidemark-fs/src/mount.rs:127`. Reference `finding:w1-gate-display-helper-duplicate`.

The helper bodies implementing Unicode-control, invalid-byte and backslash escaping are identical. This defeats the requested single definition and invites drift in security-sensitive terminal output.

**Change/owner:** coordinator/core path owner plus W1-A should provide one approved shared byte-display helper, keep RawPath/RawName wrappers delegating to it, and use it for mount fields. Preserve current renderer regressions. Related NOATIME retry code also exists in walk/getdents, probe and store/space_guard, but those callers have different Gate/fallback-count semantics; do not merge them blindly.

## Cross-task consistency and safety

- Shared types are not redeclared: DevId/Ino/Gate/GateStats and device/sample/snapshot types originate in core; throttle re-exports GateStats. XfsGeometry/BulkInode live in sys and are used by fs. StoreError has one store definition/re-export. Probe display DTOs and store's two-field FsTotals have separate purposes. G8 is the exact helper duplication found.
- Walker calls real mount discovery and per-worker ioprio; ioprio failure is logged as best effort. Discovery uses no-follow component opens, and scan metadata statx/open/getdents calls are charged before calls (including NOATIME retry/EOF). It supplements explicit skip_mounts with discovered targets; device/inode/mount checks catch replacements and cross-device children. G4 qualifies malformed-table fallback safety. Directory fd cap is supported by the scale test, not extrapolated to every adversarial schedule.
- Probe calls real xfs geometry/bulkstat support and block-device traversal, explains errors as unknown, and uses NOATIME with EPERM fallback. LVM remains an explicit W4-A unknown rather than calling its stub. Fanotify probe and CLI PostgreSQL probe still call W5-A/C stubs and report unknowns. These are assigned future implementations, not falsely successful Wave 1 results.
- Caller resolves leaf devices then injects read_disk_stats into gate_from_config. read_disk_stats preserves input order/duplicates and fails the whole sample on error, matching the accepted sampler assumption. Device-discovery failure uses the conservative fixed fallback. No blockdev call remains erroneously treated as a Wave 0 stub in this seam.
- Walker/procfs/state accounting use st_blocks * 512; BulkInode.disk_bytes is already converted bytes. Disk-stat sector counters remain sectors; controller uses tick milliseconds and interval milliseconds, not sector counts. Snapshot/sample timestamps are Unix seconds; phase/duration/gate fields explicitly use milliseconds. Current v5 allocation conversion still needs both VM oracles; old prose is G1.
- All eight XFS production unsafe blocks and the ioprio block have true comments for current private call sites: live borrowed descriptors, initialized/aligned geometry/header storage, bounded buffers, validated counts and offsets. v5 storage is zero-initialized 8+24*batch u64, batch <=65,536; only checked records are read. Layout/mutation checks validate declared offsets, not a newly measured Rocky ABI. Test-only unsafe statx is confined to tidemark-sys. CI's syntax-aware scan and source search find no production unsafe outside sys.
- No target regular-file contents are read by Wave 1 scanning; reads of proc/sys discovery data and Tidemark's own databases are separate. Procscan intentionally stats kernel /proc/PID/fd magic links to inspect already-open files, never reads their target contents, and checks PID reuse/descriptor identity. No production mount/LVM/host-configuration mutation was found. Store writes remain its state databases/scratch/recovery artifacts; permission changes concern its own files.
- Paths and names stay bytes for traversal, hashing, storage and glob matching; display escaping is tested. Discovery symlink canonicalization is explicit query resolution before scanning, not a scan following an entry. Tree algorithms are iterative. Walk tasks have count/path-byte caps, reader/proc results have documented caps and attribution cache capacity is bounded; G3 records why finite counts do not prove the total budget. Store space traversal has a documented 4,096-depth fd cap and excludes same-device binds via fdinfo mount IDs. Its own-state root opening is not the walker's component-safe scan-root API; it should not be reused for arbitrary scan paths.
- Output lists/ties are byte-sorted where specified. Walker discovery IDs/arrival order are intentionally nondeterministic and require W2-A preorder renumbering. Retention/compaction/age logic take injected timestamps; monotonic timing remains for waiting, metrics and TTL. No recursion proportional to directory depth was found.
- New public surfaces match accepted owner additions: MountRoot, catalog::recover_catalog and Catalog::recovery_files. Owner-local error variants, CompactStats/SpaceVerdict and documented caps fill planned owner types. No additional public FixedGate constructor survives. G1/G2 are the remaining coordinated public-contract preparation gaps identified here.

## Throttle conservation and lifecycle

While unit costs remain exactly representable (G7), earned credit advances only by old rate × elapsed time and is capped at issued + quarter-second burst. A rate increase does not mint credit; decrease min-clamps it. Each caller reserves a cumulative target, so newer calls cannot consume older reservations. Notifications and condition waits use the same mutex and every timeout/notification rechecks debt; no lost-wake or lock-order cycle was found. Timed waits progress without rate updates; OS scheduling fairness is not proved.

Production constructors normalize nonpositive/invalid rates to at least one; max profile intentionally uses NoopGate. Zero cost returns immediately. stop wakes/joins the controller but does not cancel acquisitions; they continue at the retained final rate. Its bounded join depends on the accepted synchronous sampler finishing within its interval; arbitrary forever-blocked callbacks are not supported. Independent source-copy probes cover rate increase, normalization and progress after stop in addition to the four original throttle tests. Duration waits are capped; aggregate blocked_ns can wrap after approximately 584 worker-years of blocked time, not a tested endurance guarantee.

## Wave 1 gate checklist

| Checklist item | Status | Evidence / action |
|---|---|---|
| Five branches approved and merged in order; CI green | NEEDS-HUMAN (local CI PASS) | Implementations/fixes are present in gate tree, but handoffs/reviews are not approval receipts. No branch provenance/approval inference made. tools/ci.sh exits 0; cargo-deny explicitly skipped. Human/coordinator verifies merge approval/order and completes audit. |
| Privileged suite green on Rocky 8.8 and 8.10 | NEEDS-HUMAN | Feature-enabled test build passes; none executed. G6 fixture setup and v5 policy below must be resolved. |
| Recorded v5 availability, bs_blocks unit, default scheduler | NEEDS-HUMAN | No runtime findings supplied in tree. Required destination docs/handoff/W1-gate.md is absent and outside this report's write scope. |
| No duplicated helpers; no unsafe outside sys | FAIL | Unsafe placement/comments pass inspected code/CI; duplicate byte-display helper G8 remains. |
| Tag wave-1-done | NEEDS-HUMAN | Only after fixes, independent approvals and platform gate. No tag, merge or push performed. |

## Waves 2 and 5 readiness

**Do not start all seven tasks in parallel from this tree.** First complete G1/G2 and a coordinated shared-file preparation; address resource/boundary follow-ups before depending on scan safety. W2-A's signature-first compiling aggregator skeleton is itself an explicit serial prerequisite for W2-B/C. W5's API prerequisites are present; its dependencies/modules still need preparation and wave-entry approval.

| Task | Wave 1 functions/types available | Mismatch or required preparation |
|---|---|---|
| W2-A | core scan records/rows; walk(&WalkConfig,&dyn Gate,&AtomicBool,Sender<WalkOutput>) -> Result<WalkSummary,WalkError>; store StoreError | Shapes match. Use gate tree's tie fix, not blind candidate worker. Aggregator signatures are W2-A's own missing skeleton, not an omitted W1 implementation. Resolve G3 before memory acceptance. |
| W2-B | walk; read_dir_entries(&RawPath,&dyn Gate) -> io::Result<Vec<(RawName,Ino,FileKind)>>; xfs is_xfs/geometry/bulkstat_probe/ag_ino_range/bulkstat and BulkInode | Rust shapes exist; v5-only semantics conflict with old plan. Gate must charge **before** iterator.next; first next hides geometry plus bulkstat, later/terminal/error calls also require charging. Agree conservative batch precharge rather than charge only returned n after I/O; do not multiply disk_bytes again. |
| W2-C | canon, mount queries, blockdev resolution/sampling, gate factory; SnapshotWriter put/finish/abort/space callback; complete Snapshot queries; Catalog selection/record/rebuild; TimeSeries::insert_dir_series; retention_plan; space_check | G1/G2. MountInfo.root is enum, not RawPath. CLI must canonicalize original PathBuf before ScanRequest.root becomes RawPath. Explicit recovery path is tidemark_store::catalog::recover_catalog, requiring closed connections; G5 determines command ordering. Caller filters series_depth, installs writer space checks and implements prune/retry. |
| W5-A | AttributionCache::new(Duration,usize), lookup(pid) -> Attribution; proc helpers; sys fanotify::probe(&Path) -> io::Result<FanotifyCaps> stub | Existing W1 signatures match. Fanotify event APIs are W5-A-owned implementation, not missing W1 functions. |
| W5-B | procfs::scan_open_files(&[DevId],bool) -> io::Result<Vec<OpenFile>>, all fields and attribution methods | Shapes match. Preserve raw target bytes; fake proc source abstraction is this task's responsibility. |
| W5-C | core PostgresConfig/PgSample/PgNow; pg::probe_from_config(&PostgresConfig) -> Result<PgProbe,PgError> stub | No missing W1 prerequisite. Collector/query/label internals are W5-C work; CLI already imports pg. |
| W5-D | core live/config/rules types and default_rules; rates slope_bytes_per_sec/time_to_full stubs | No missing W1 prerequisite. window_rate, alerts and now are W5-D work. Keep shared input types core-owned; avoid live→pg dependency. |

### Shared-file preparation inventory

Neither scan nor live currently attaches any dependencies; pg attaches only core/thiserror. All packages below use existing workspace entries. Names omit the `tidemark-` prefix for internal crates in this table.

| Task / crate | Missing dependencies forced by named interfaces or planned algorithms | Other concrete choices to settle in prep |
|---|---|---|
| W2-A / scan | core, store, thiserror, rustc-hash; dev test-support, proptest | tempfile for direct temporary fixtures |
| W2-B / scan | fs, sys, crossbeam-channel, rayon, smallvec (shared A dependencies above) | privileged forwarding to test-support; safe syscall/locking wrapper choice |
| W2-C / scan | globset plus shared core/fs/sys/store/channel/error above | serde for outcome serialization, tracing diagnostics, rustix or standard safe fd/lock APIs, dev tempfile |
| W2-C / CLI | scan, store | globset/channel/rustix only if direct top --live code lives in CLI rather than scan helper; select safe SIGINT handling, since workspace has no signal-hook/ctrlc entry |
| W5-A / live | core, fs, sys, thiserror; dev test-support | bounded crossbeam-channel, parking_lot, rustc-hash, tracing; safe poll/eventfd in sys using existing libc or rustix/event attachment; tempfile |
| W5-B / live | core, fs, thiserror shared above | shared cache/channel/diagnostic choices; tempfile |
| W5-C / pg | postgres (core/thiserror already attached) | tracing, tempfile for pgtmp; serde only if task DTOs require it |
| W5-D / live | globset, tracing-journald, ureq; dev tiny_http, insta | serde_json/serde for webhook DTOs, tracing, shared channel/locking attachments |
| W5-A/B / CLI | live | Existing CLI assert_cmd/tempfile/test-support/insta cover planned testing |
| W5-C / CLI | none for pg, already attached | Existing generic CLI dependencies suffice |

One preparation owner should edit `crates/{tidemark-scan,tidemark-live,tidemark-pg,tidemark-cli}/Cargo.toml`, Cargo.lock, and privileged feature forwarding. Workspace Cargo.toml needs changes only for chosen extra features/dependencies (e.g. rustix/event or a justified safe signal implementation); no OpenSSL/async runtime. Recommended attachments are implementation choices, not falsely claimed mandatory API omissions.

Shared module/export files: `tidemark-scan/src/lib.rs` needs coordinated root exports for run_scan, request/outcome/recorder/error, mode/progress and any root-qualified SinkError; `tidemark-pg/src/lib.rs` may need expanded/re-exported PgError and collector types; `tidemark-cli/src/render/mod.rs` needs tree/scan_progress declarations and compiling placeholders. Existing scan/live/sys/fs crate roots already declare their task modules, and CLI cmd/mod.rs already declares all seven tasks' commands. Existing scan agg/mod.rs (lca/linkset/notable/rollup), bulk/mod.rs (join/names), live watch/mod.rs (fdmode/fidmode/rings), alerts/mod.rs (deliver/rules) and pg lib.rs (collector/labels/queries) already declare children; their implementation edits stay with their respective task owners, not competing tasks. Store/lib.rs needs no edit when recovery imports are catalog-qualified. G2 may add core/scan.rs or fs/lib.rs export work only after contract approval. W5-D can re-export core default_rules in its owned alerts module.

## Open requests and gaps

| Request / gap | Verified status |
|---|---|
| Root SCR-001 through SCR-006 | Accepted/modification decisions are in docs/plan/13; its opening overrides historical Pending text. No new open request inferred. |
| scr-W1-A/SCR-001 | File says Pending; brief/fix authorization and implemented MountRoot answer it. Decision transcription into plan remains G1. |
| scr-W1-A/SCR-002; A/B handoff CLI assertion requests | Answered in code: stale probe exit-1 assertion removed in surface.rs:209; CLI-surface-test-fix handoff exists; CI passes. SCR file's Pending label is stale. |
| scr-W1-D/SCR-001 | Accepted in task/fix evidence, still Pending in file. Stable positional samples, whole-sample failure, utilization masking, bounded callback lifetime and caller-owned discovery need plan transcription. Old note assigns discovery to W2-B, but W2-C is the actual orchestrator owner. |
| scr-W1-E/SCR-1 and globset/filter asks | Explicit recovery and matcher/dependency implemented; old Pending/unimplemented notes superseded by fix. Ordinary-open/preservation conflict is G5. |
| Remaining “Needs from coordinator” | A: both VM runs, real mountinfo captures, disposable md fixture. C: both VM runs/v5 findings and equipped-host cargo-deny. D: both privileged tests, genuine EL8 cgroup captures, cargo-deny and contract transcription. E: both privileged tests and cargo-deny. B's old CLI ask is answered; later Rocky/container runtime still unperformed. |
| Source TODO/FIXME/XXX | `rg` across crates, xtask and tools found none. Future-owner stub files are explicit scheduled work, not unreferenced TODOs. |
| Other disclosed gaps | No actual Rocky header remeasurement/other-architecture ABI validation; no production database-server throttle load test; no forced EPERM fallback/end-to-end permission/EIO regression; no power-cut store durability experiment; no previous schema version exists to test backward compatibility. Socket-fixture behavior is host-dependent. LVM probe is W4-A; fanotify/PG are W5-A/C. No claim that those later features work. |

Historical handoffs describe superseded implementations (v1 fallback, immutable deadlines, glob unavailable, CLI failure); code and newer fix evidence take precedence for this review. Genuine EL8 input captures remain missing despite local green tests.

## For the human gatekeeper

Assumed SSH aliases are `rocky88` and `rocky810`, following xtask examples; no host identities were verified. After the preparation/fixes above, the exact supported runner commands are:

```sh
cargo xtask vm-test --host rocky88 --remote-dir tidemark-wave1-review --suite cargo
cargo xtask vm-test --host rocky810 --remote-dir tidemark-wave1-review --suite cargo
```

Run these only against disposable authorized VMs. The runner builds privileged tests and runs them serially under sudo. Supply the pinned toolchain/cache on each VM if offline, ordinary documented XFS/LVM/compiler/SSH prerequisites, plus ionice, setpriv, Python 3 and lsblk. Resolve G6's md environment first. Do not infer that setting a variable on the invoking development host configures the remote root test process.

Record kernel/xfsprogs versions, **v5 bulkstat availability**, the 10,000-inode **disk_bytes == statx.stx_blocks × 512** results establishing bs_blocks conversion, and **active default scheduler on each resolved virtual leaf** (probe JSON) for both VMs in `docs/handoff/W1-gate.md`. Retain passing test output separately using VM-local --nocapture/--show-output or an improved runner; plain successful runner output hides the v5 diagnostic.

The current v5 probe test asserts both v1 and v5, while the product degradation contract permits walk when bulk is unavailable. If a supported Rocky kernel lacks v5, the suite fails even though walk-only operation may be acceptable. Human/coordinator must explicitly decide whether that is a platform blocker or a supported fallback and update acceptance/tests accordingly; never restore truncated v1 metadata. Also decide G1/G2 API amendments, G3's memory/refusal contract, G5 recovery ordering, and review/merge/tag approval. Collect real EL8 mountinfo/cgroup fixtures. Run cargo-deny on an equipped host. This reviewer performed none of these privileged or approval actions.

### Every current Wave 1 test pending VM

All sixteen below compile, are gated by the privileged feature, and are not ignored. Assertions are read from current code, not copied blindly from handoff counts.

| Owner / test | Exact assertion to validate |
|---|---|
| A — probe_loop_xfs_geometry_matches_xfs_info | Report fs_type=xfs; reported blocksize/inodesize equal parsed xfs_info fields. |
| A — resolve_loop_lvm_returns_pv_disk | Exactly one leaf; its major/minor equals the PV device from lvs devices output. |
| A — resolve_vm_md_array_matches_lsblk_members | Configured md level begins raid; inverse lsblk yields >=2 disk/loop leaves; exact major:minor set equals resolve_leaves. |
| A — real_bind_mount_is_a_same_device_submount | Real bind target appears in submounts_under and same-device other_mounts_of_dev. |
| B — noatime_no_writes | Strictatime root/child atime seconds and nanoseconds unchanged; noatime_fallbacks=0. |
| B — real_bind_mount_is_skipped | Exactly root and source directories traversed; bound entry recorded as Skip(Mount). |
| B — ftype_zero_walk_and_namespace | crc=0/ftype=0 walk dirs=2, entries=3, errors=0 and independent metadata equality; namespace dirs=2, total nondirectory children=2. |
| C — geometry_matches_xfs_info | is_xfs true; AG count/size, inode/data/directory block sizes, ftype match xfs_info; fixture v5/reflink true; UUID nonzero. |
| C — bulkstat_covers_all_inodes | Strict ascending inode order; namespace-visible map of ino to nlink,size,mode,mtime equals nofollow statx oracle, including negative mtime. |
| C — bulkstat_ag_ranges_partition | Every per-AG result inside its half-open range; concatenation equals full iteration; no duplicate inode. |
| C — bulkstat_blocks_match_statx | All 10,000 synchronized varied regular/sparse files match disk_bytes to measured stx_blocks*512. |
| C — bulkstat_probe_records_kernel_support | Prints kernel/v1/v5; asserts both support flags and nonempty bulk iteration. Five current C tests; historical sixth forced-v1 test was correctly removed. |
| D — ioprio::tests::idle_class_as_root | Worker idle-priority call succeeds; ionice query of worker TID succeeds and starts with idle. |
| D — root_sees_other_user_open_file | Parent UID0; UID/GID65534 child succeeds; proc scan finds exact reported PID/fd and measured inode. |
| E — snapshot_xfs_publication_owner_permissions_and_sync | Published uid/gid0, mode0600, reopened root id0; does not simulate a power cut despite its name. |
| E — state_usage_skips_same_device_bind_mount | State containing bind-to-large-file refuses only mandatory free-space floor, not state allocation cap, demonstrating payload skipped. Cap uses measured root blocks plus 4096 slack; this is not an asserted filesystem allocation size. Prefer deriving any future slack from the measured empty fixture. |

The unprivileged `captured_el8_inputs_pending_vm` is an additional pending input check, not a privileged test: it currently returns early. Once captures exist it requires 3–4 files and parses each nonempty. Relevant fixture/gate counts (e.g. 10,000 files or two directories) are structural counts; reviewed allocated-byte expectations use metadata/oracles rather than fixed filesystem-dependent allocation values. Synthetic SQL rows/layout constants and the 512-byte Linux stat block unit are not assumed XFS allocation sizes.

## Build, test and probe results

Environment: pinned Rust 1.98.0; `CARGO_HOME=/tmp/tidemark-gate-cargo` copied from host registry without modifying it; `CARGO_TARGET_DIR=/tmp/tidemark-gate-target`; `CARGO_NET_OFFLINE=true`. No crates.io/VM access or privileged test execution.

| Check | Observed result |
|---|---|
| tools/ci.sh | Exit 0: source-owner/read-only checks, formatting, all-target Clippy with warnings denied, workspace tests/doctests. Log reports 162 passes, zero failures, three ignored scale tests. Cargo-deny unavailable and explicitly skipped. |
| cargo build --offline --workspace --features privileged --tests | Exit 0; compilation only. |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | Exit 0. |
| Explicit ignored walker wide-directory scale test | Exit 0, 4.19 s; measured RSS delta 1,683,456 bytes, peak fds6 versus baseline5/limit12. No notables retained in this test. |
| Explicit release store 2M-row scale test | Exit 0; insert phase 5.052826411 s (<60 s); whole test including publication 8.20 s. `store-scale.log/.exit`. |
| Old/new sample SQL comparison | 6,000 pairs identical, all fs/LVM/PG query families; host SQLite 3.53.4. Actual production constants also tested against bundled SQLite in CI. |
| Throttle exact-source probes | Seven tests passed, including deliberate precision-defect reproduction, rate increase/zero/stop and four retained regressions. |
| ABI tests and mutation | Original four tests pass; UID/GID mutant fails offset guard as expected (exit101). |
| Exact candidate walker replay | Cross-directory tie regression fails at d8f130fe (exit101); current gate walker passes both tie regressions. |
| Extra parser/glob/recovery probes | nsfs/malformed-neighbor tests pass with real-capture SKIP disclosed; byte glob probe passes; ordinary-open sidecar loss reproduced. |

No VM, real XFS ioctl/loop mount, LVM modification, service enablement, kernel/scheduler adjustment, production-load experiment or power-cut test was performed. Not all hypothetical failure interleavings were exhaustively modeled. ABI/runtime compatibility, human decisions, advisory audit, actual EL8 captures and the listed new findings remain open. The result is a review proposal for the coordinator and human gatekeeper.
