# 13 — Contract decisions

This record also covers the accepted Wave 1 requests and reviews below. The coordinator accepts every finding in `docs/reviews/W0-B1-review.md` and decides SCR-001–004 **MODIFY (accepted with modifications)** and SCR-005–006 **ACCEPT** as specified below. This decision record takes precedence over the “Decision: Pending” lines in `docs/handoff/SCR-001.md` through `SCR-006.md`. These are the decided contracts for later tasks; the implementation changes are a separate task.

## SCR-001 — Snapshot types, filters and bounds

The SQL schema left Rust widths, metadata JSON shapes and filters undefined. Nullable bulk file names also made the promised always-present full path impossible, and core cannot depend on the later filesystem gate implementation.

**Decision: accepted with modifications.**

- Directory ids/depth, direct entry/error counts, link counts and project ids are `u32`; inode/device ids, sizes and subtree counters `u64`; Unix seconds/path hashes `i64`; errno `i32`. SQL nullability maps to `Option`. Directory names are `Vec<u8>` (root holds full path), other names `RawName`, paths `RawPath`. All explicit scanner widths remain intact.
- Notable-file ids are zero-based `u32`, separate from directory preorder ids. Validate `1 <= max_notable <= u32::MAX`, check id assignment, and check every SQLite integer conversion in both directions (including signed SQLite range for `u64`). Bulk slots must fit 31 bits (`slot <= 0x7FFF_FFFF`) before setting the self-reference flag. W2-A internal depth is changed to `u32`, so it never truncates core depth.
- `SnapshotMeta` includes every required schema key in documented order: schema_version/threads `u32`, counts/sizes/devices `u64`, timestamps `i64`, scan_id `ScanId`, root `RawPath`, mode `ScanMode`, status `SnapshotStatus`, degraded `bool`, exclude `Vec<String>`, link_instances `LinkInstances`; remaining descriptive scalars `String`, including fs_uuid empty when unknown. `phase_ms: BTreeMap<String,u64>` is deterministic. `gate_stats: core::scan::GateStats` has mean_rate/min_rate `f64`, throttled_ms/decreases/increases `u64`; `tidemark_fs::throttle` re-exports it.
- `FileFilter { min_size: Option<u64>, older_than: Option<Duration>, newer_than: Option<Duration> }` measures ages from snapshot started_at. `LinkFilter { incomplete: bool, boundary: Vec<String>, boundary_depth: Option<u32> }` rejects simultaneous boundary globs/depth. `FileHit { row: FileRow, path: Option<RawPath> }` preserves unresolved bulk-race names honestly. `LinkSetHit { row: LinkSetRow, lca_path: RawPath, instance_count: u64 }` counts stored instance rows, not discovered found. Bulk link-instance names are always absent by design and resolved live on demand.
- Keep the eight single-snapshot `SortKey` variants in `05` §3. CLI sort uses a wrapper with `Growth`; W2-C compares two snapshots with raw-byte path tie breaking (`08` §1.2).

Plan locations: `04` §3.1; `05` §1–3; W0-B, W1-D/E, W2-A/C. Affected tasks: W0-B2, W1-B/D/E, W2-A/B/C, W3-A/B/C.

## SCR-002 — Shared API payloads and wire typing

Core payloads referenced types owned by crates that themselves depend on core. Request root encoding, scalar types and writer-window semantics were not fully defined, while report serialization could expose integer byte arrays.

**Decision: accepted with modifications.**

- `core::api::ComponentState { name: String, state: String /* ok|degraded|disabled|error */, detail: String }` and `VgInfo { vg: String, size: u64, free: u64 }` are the shared definitions. Re-export them as `tidemarkd::firstboot::ComponentState` and `tidemark_fs::lvm::VgInfo`.
- Request version is `u32`, id/method `String`, required params `serde_json::Value` (`{}` for no arguments). Response version/id match, ok is bool, optional result/error are omitted when absent. `ApiError { code: ApiErrorCode, message: String }` retains `bad_request|forbidden|not_found|unavailable|busy|internal` snake_case codes. All methods have named params/results; array/optional results alias their normative live payloads. Limit `usize`, uptime_s `u64`, sampled_at `i64`, mount/group labels `String`.
- Start/cancel require exactly one `root` UTF-8 string or `root_b64` padded standard base64; status permits at most one, omission meaning all roots. Reject conflicts, invalid base64 and invalid `RawPath` inputs through validated DTO adapters. Internal byte serde may remain behind those adapters.
- Shared report DTO adapters emit mutually exclusive `path` UTF-8 strings or `path_b64`, including nested phantom holders, snapshot paths and path-bearing directory hot entries. Preserve internal byte fields. Test full DTO/envelope round trips for ASCII, Unicode, invalid UTF-8 and absent optional roots.
- Writers use positive `Duration` with humantime serde; hot retains `HotWindow` (1m/5m/15m/60m) and `HotBy` (dir/unit/comm). API/CLI scan override enums remain Auto/Walk/Bulk and Gentle/Normal/Max; custom throttle names are configured only through root throttle strings.
- Explicit `mount: String`/`ScanStatus.root: String` fields override the general raw-path sentence. Configured mounts/roots are exact UTF-8 TOML paths. Arbitrary non-UTF-8 request roots displayed in String-only status use `RawPath::display_lossy()` byte escaping, not replacement characters; this is display, not a request encoding.

Clarifications where the recommendation leaves a choice: no additional writer-window maximum is imposed. For growth sorting, compare the selected snapshot with the older complete snapshot used for Δ24h, descending signed disk-byte delta, with an error if that older snapshot is unavailable. These fit the existing CLI surface without new flags.

Plan locations: `08` §1–2; `07` §7a; W0-B, W2-C, W4-A/B, W6-A. Affected tasks: W0-B2, W4-A/B, W5-B/D, W6-A and other report renderers; W1-E/W5-B also follow S6.

## SCR-003 — Effective configuration and default rules

Configuration examples omitted some representation and required-field policies. Default-rule generation in live would introduce a dependency cycle, and order-dependent names would make overrides unstable.

**Decision: accepted with modifications.**

- Config/table structs deny unknown fields. `ConfigIssue { field: String, message: String }` contains dotted-field validation errors. Paths `PathBuf`, sizes `ByteSize`, ordinary durations `Duration` with humantime serde, bounded counts `u32`/`usize`, percentages/latencies `f64`; mode/link_instances are enums. Root throttle is `String`; profiles `BTreeMap<String, ThrottleProfile>` merge overrides with gentle/normal defaults. Reject definitions named `max` (reserved NoopGate selector), while allowing root throttle `max` and custom profiles. API/CLI overrides keep three presets.
- Absent roots use both example roots; absent per-root window is None, built-in /data explicitly has its example window. Missing config files return their I/O error for CLI fallback. Load one file with `MAX_CONFIG_BYTES = 1 MiB`. Default/deserialization produce effective defaults; merging is idempotent.
- `AlertKind` is an enum; name/kind required, enabled defaults true, for_/resolve_for durations default 5m/10m (`for_` renamed `for`). `above: Option<AlertThreshold>` is untagged Number(f64)/Text(String); below/window/older_than are optional strings validated by kind. Require every rule-specific field in `08` §3's kind usage set except applicable vg/pool globs (effective `*`); reject unused or missing fields even for disabled rules. `scan_stale` requires root/older_than. Numeric hot_dir_growth above is finite nonnegative bytes/s, equivalent to a rate string; watcher count uses `above`, never a second `overflows` key.
- Pure `core::config::default_rules(cfg)` owns generation; W5-D wraps/re-exports. Keep eight default kinds and thresholds/windows/hysteresis in `07` §6.4. Filesystem/phantom rules expand over sampler mounts, watcher rules over watch mounts; stale defaults expand over configured roots except schedule off. Explicit stale rules can monitor manually scanned roots.
- PostgreSQL-specific default names in this historical decision are superseded by ADR-019; the remaining names and merge semantics still apply.
- Base names: fs_used_pct, fs_full_soon, phantom_bytes, watch_lossy, pg_slot_retained, pg_archiver_failing, scan_stale, thin_pool_pct. Only exact /data gets unsuffixed mount/root names; all other subjects append `:<mount>`/`:<root>`, independent of ordering. User rules override same-name defaults; duplicate user names survive merge for validation.
- Collector/input-disabled or unavailable rules remain visible in effective config but do not fire. Thin-pool rules evaluate only tracked thin LVs. Metrics listen is empty (disabled) or a parsed SocketAddr with loopback IP; reject wildcards/external addresses.

Chosen suffix escaping to settle S3: preserve `/` and ASCII `[A-Za-z0-9._-]`, percent-encode all other UTF-8 bytes as uppercase `%XX`, including `%` and `:`. This is byte-preserving and independent of mount order.

Plan locations: `08` §3; `07` §6; W1-D, W5-D. Affected tasks: W0-B2, W1-D, W2-C, W4-B, W5-D, W6-A/B.

## SCR-004 — Helpers, formatting and scan ids

Helper signatures and construction/error policies were unspecified. The static schema field prevented ordinary owned JSON deserialization, and an epoch fallback for invalid scan dates would misrepresent the supplied timestamp.

**Decision: accepted with modifications.**

- `RawName/RawPath::new(bytes: impl Into<Vec<u8>>)` and `from_path(&Path)` return `Result<Self, CoreError>`; `as_path() -> &Path`, `join(&RawName) -> RawPath`, parent/file_name return owned Options, starts_with takes `&RawPath`, display_lossy returns String, hash64 i64, to_json serde_json::Value. RawName rejects empty/slash/NUL/dot/dot-dot; RawPath rejects NUL/nonabsolute inputs and lexically normalizes repeated slashes/dot/dot-dot, clamps above-root parents, removes trailing slash except root. Prefixes are component-wise, ordering byte-wise. Lexical normalization performs no filesystem I/O: callers canonicalize live query paths first where CLI promises realpath.
- Internal serde preserves byte arrays with validated deserialization. Display escapes controls, literal backslashes and invalid UTF-8 with uppercase byte escapes; printable Unicode is preserved. JSON path_b64 uses padded standard base64. Hash is xxh3_64 bits reinterpreted as signed i64.
- ByteSize is a u64 newtype, accepting integers and checked decimal/binary-unit strings, explicit decimal SI suffixes, and fractional byte truncation. Parsers are fallible; parse_rate returns `Result<f64, CoreError>` bytes/s. `fmt_bytes(u64)` prints exact bytes below 1 KiB; otherwise three significant digits after rounding, recomputing decimal precision and promoting units. Boundary outputs include `10.0 KiB`, `100 KiB`, `1.00 MiB` for 10239, 102399, 1048575 bytes. Apply shared rounding to signed `fmt_rate(f64)` binary units/s; nonfinite inputs render `unknown`.
- Age bounds are `[i64; 5]` as W0-B lists; exact boundary belongs to older bucket. Duration helpers use Duration/CoreError; isolated unix_now returns i64.
- `ScanId::new(now: i64) -> Result<ScanId, CoreError>` emits `YYYYMMDDTHHMMSSZ-<8 lowercase hex>` and fails outside supported four-digit UTC years (0000–9999); no epoch fallback. Suffix hashes injected seconds, process id and process-local atomic sequence, with no file I/O. It is neither cryptographic nor a uniqueness guarantee. Writers refuse existing ids and retry fresh ones; callers propagate invalid-date errors.
- `DevKind::Md.level: String` (e.g. raid5), major/minor `u32`, DiskStat fields `u64`. `JsonDoc<T> { schema: Cow<'static, str>, data: T }` supports owned Deserialize without serde borrow, while output may borrow schema constants.

Plan locations: W0-B; `08` §1.1; `05` §2; W1-E, W2-C. Affected tasks: W0-B2, W1-A/E, W2-C and all later path/JSON consumers.

## SCR-005 — Stubs with error-free signatures

The generic cross-task stub rule requires `Err(NotYetImplemented)`, but some documented signatures return plain values, `Option`, references, constructors or unit. Returning an error from those signatures would change the frozen contract.

**Decision: accepted.**


- A stub whose documented signature can return an error returns `Err(NotYetImplemented)`.
- A stub whose documented signature cannot return an error keeps that exact signature and returns an inert placeholder: `None`, an empty collection, zero or a default value. The signature in the wave file always wins over the generic stub rule.
- Callers treat these functions as "unknown" until the owning task merges; placeholders must not be treated as real data. The owning task replaces the body without changing the signature.

Plan locations: W0-A deliverable 3 and W0-B Cross-task stubs; W1 entry; `05` §3; W4-A; W5-D. Affected tasks: W0-B3, W1-A–E, W4-A, W5-D.

## SCR-006 — `du` apparent-size oracle

The normative `t_apparent` definition includes directory `st_size`, but GNU `du` apparent mode differs across releases. In a `rockylinux:8` container, `du (GNU coreutils) 8.30` was measured to count directory sizes; coreutils 9.2 and later count only regular files and symbolic links in apparent mode.

**Decision: accepted.**


- Keep the directory-inclusive `t_apparent` definition in `03` unchanged. Direct equality with `du -sxB1 --apparent-size` holds when `du` counts directory sizes, including on the target platform; NFR-1 is unchanged in meaning.
- Compare `t_disk` directly with `du -sxB1` on every version.
- For `t_apparent`, probe the local `du` with an empty directory to learn whether apparent mode counts directory sizes. When it does not, add the measured `st_size` of the subtree's directories (including its root) and any other non-regular, non-symlink inode with non-zero `st_size` to the `du` figure before comparing. Count distinct inodes consistently with `03`; the probe decides, never a version string.

Plan locations: `03` §2–3; `09` §3; `01` NFR-1; W2-C oracle tests. Affected tasks: W0-C, W1-B, W2-A, W2-C.

## Accepted review findings

| Finding | Decided contract and plan location |
|---------|------------------------------------|
| B1 | Metrics listen must parse and be loopback, or empty/disabled: `08` §3 (W6-B already restricts loopback). |
| B2 | Shared whole-report adapters emit path/path_b64, preserving internal bytes: `08` §1.1/§2, `07` §7a, W0-B/W6-A. |
| B3 | Stale rules require root; generated defaults only for scheduled roots: `08` §3, `07` §6.1/§6.4, W5-D. |
| B4 | Finite nonnegative numeric hot growth means bytes/s, alongside rate strings: `08` §3, `07` §6.2, W5-D. |
| B5 | Three significant digits after rounding/unit promotion; exact bytes below KiB; shared rate fix: W0-B and SCR-004 above. |
| S1 | Writers accept positive humantime Duration; hot keeps enumerated windows: `08` §1.2/§2. |
| S2 | JsonDoc schema is Cow, owned Deserialize without borrow: `08` §1.1, W0-B. |
| S3 | Stable /data-only unsuffixed names, byte-preserving suffixes: `07` §6.4 and SCR-003 above. |
| S4 | Zero-based u32 notable ids, validated cap and checked assignment/SQL conversions: `05` §1/§3, `08` §3, W1-E/W2-A. |
| S5 | Reject profile definitions named max; keep selector/custom profiles: `08` §3, W1-D. |
| S6 | phantom_samples/writer_samples comm/path NOT NULL; retain required live types; never silently substitute empty values for unknowns: `05` §6, `07` §7a. |
| S7 | CLI Growth wrapper compares two snapshots, raw-path ties; snapshot enum unchanged: `08` §1.2, `05` §3, W0-B/W2-C. |
| N1 | Notable-file row id; bulk instance names absent by design, resolved live: `05` §1. |
| N2 | Watcher threshold key above (overflow count): `07` §6.1. |
| N3 | Alert state firing/resolved/renotify: `05` §6, matching `07` §7a. |

---

## Wave 1 decisions

These final coordinator decisions supersede pending request text in the Wave 1 handoffs. Implementation details cited below follow the fix notes; module inspection is not runtime verification.

| Wave 1 request | Status | Decision section |
|----------------|--------|------------------|
| `scr-W1-A/SCR-001` | **Decided** | Mount roots; G4 |
| `scr-W1-A/SCR-002` | **Decided** | CLI surface tests |
| `scr-W1-D/SCR-001` | **Decided** | Throttle; G2/G7 |
| `scr-W1-E/SCR-1` | **Decided** | Store; G5 |

The request files themselves remain unchanged.

### Mount roots

Kernel-object mount roots from nsfs are not paths. Rejecting them must not invalidate other mounts in the table.

**Decision: accepted.**


`MountInfo.root` is `enum MountRoot { Path(RawPath), Object(Vec<u8>) }`. Object roots such as `net:[4026533000]` remain mount boundaries. Preserve independently decoded boundaries and fail closed as specified in G4 below.

Plan locations: W1-A interface/details; `04` §2.2. Affected tasks: W1-A, W1-B, W2-C.

### Query paths

Lexical normalization before realpath can change what a symlink-containing path resolves to. Unresolved paths must not borrow another mount’s facts, and printable UTF-8 still needs safe escaping.

**Decision: accepted.**


Canonicalize the user-supplied path (resolve symlinks as realpath) before constructing `RawPath`; never lexically normalize first. Return unknown facts with the reason on resolution failure. Escape every displayed mount field like `RawPath::display_lossy`, whether valid UTF-8 or not.

Plan locations: W1-A details; SCR-004 above. Affected tasks: W1-A, W2-C, later query renderers.

### Bulk mode requires v5 bulkstat

Legacy v1 bulkstat has a 16-bit link count: 65,537 arrives as 1 without detectable overflow. Exact link counts determine reclaimable space (`03` §4).

**Decision: accepted.**


`tidemark_sys::xfs::bulkstat` uses `XFS_IOC_BULKSTAT` v5 with 32-bit `bs_nlink` only, returning `Unsupported` when unavailable. `bulkstat_probe` still reports `BulkstatSupport { v1, v5 }`. Auto chooses bulk only with v5; explicit `--mode bulk` without it fails with exit code 5 and a message naming the reason. Walk uses exact `statx` counts.

The EL8 userspace header (`xfsprogs-devel-5.0.0-12.el8`, `/usr/include/xfs/xfs_fs.h`) lacks the v5 structures and `XFS_IOC_BULKSTAT`. Module-binary inspection of `xfs.ko` in `kernel-core-4.18.0-477.27.1.el8_8` (Rocky 8.8) and kernel 4.18.0-553.172.1.el8_10 (Rocky 8.10) found `xfs_ioc_bulkstat`, `xfs_bulk_ireq_setup` and `xfs_bulkstat_to_bstat`. First real runs on 2026-10-12 establish successful v5 ioctl execution on both releases (13 R4-15); independent block-unit and scale gates remain.

Bindings are hand-transcribed, not bindgen output, and pinned by layout tests to sizes and offsets measured with `gcc`: `xfs_bstat` 136 bytes, `xfs_bstime` 16, `xfs_fsop_bulkreq` 32, `xfs_fsop_geom` 112 from the EL8 header; `xfs_bulkstat` 192, `xfs_bulk_ireq` 64 and `XFS_IOC_BULKSTAT` `0x8040587f` measured against `xfsprogs-devel-6.4.0-7.el9` (same kernel ABI).

Plan locations: `04` §5/§7; W1-C notes/gate; W2-B/C; `12` R4/R7. Affected tasks: W1-A/C, W2-B/C, W7-B.

### Throttle

Counter-only samples need stable leaf identity and meaningful utilisation. Immutable acquisition deadlines ignore rate decreases while work is already waiting.

**Decision: accepted.**


Except for the G2 factory handle below, sampler signatures are unchanged. The caller resolves leaves with `blockdev::resolve_leaves`; unavailable discovery means an empty slice and the configured minimum gate rate. `StatSampler` and `read_disk_stats` return exactly one sample per leaf in input order and fail the whole sample if any device cannot be read. Samplers finish within one interval. Signatures stay unchanged; `controller_step` is counter-only, so callers mask utilisation (`io_ticks_ms = 0`) for non-rotational, unknown or multi-queue devices. `FixedGate::new` is private.

The token bucket accounts for outstanding token debt: elapsed earnings use the previous rate before a rate change, and waiting acquisitions recheck their targets at the new rate. Per `docs/handoff/W1-D-fix.md`, cumulative issued/earned tokens use constant-size mutex/condition-variable state, with no waiter queue; surplus is capped to 0.25 seconds at the current rate.

Plan locations: `04` §6.2; W1-A/D notes; W2-C gate setup. Affected tasks: W1-A/D, W2-B/C.

### Store

Retention can otherwise remove a root’s last usable snapshot with zero recent retention. A corrupt shared database cannot be rebuilt through a catalog that failed to open, and lossy glob matching changes path meaning.

**Decision: accepted.**


Always keep each root’s newest complete snapshot regardless of policy. `Catalog::open` remains non-destructive. After an open reports corruption, callers stop writers and close all catalog/sample connections, then call `tidemark_store::catalog::recover_catalog(state_dir)` to preserve the database and WAL/SHM sidecars by rename, create a fresh database and rebuild the catalog from valid snapshot files. The daemon at startup and `snapshots rebuild-catalog` use this path only on reported corruption and log the preserved paths. Per `docs/handoff/W1-E-fix.md`, preservation uses an exclusively created private `timeseries-recovery-N` directory inside `state_dir`, with at most 10,000 namespace attempts and directory syncs; no artifacts are deleted. `Catalog::recovery_files() -> &[PathBuf]` reports the preserved paths. Historical samples are unavailable in the new database but remain in those artifacts.

`LinkFilter.boundary` globs match complete raw directory path bytes with literal separators (`*` stays within a component). Per W1-E fix, deepest matching ancestor defines a region, unmatched instances share an outside region, and sets crossing distinct regions are selected; region/pattern storage is bounded by the existing 1,000,000-row cap and errors above it.

Plan locations: `05` §3/§4/§7–8; W1-E; W2-C; W6-A startup. Affected tasks: W1-E, W2-C, W3-B, W6-A.

### Notable files

Worker-local filename tie breaking can discard the global winner before final directory ids are assigned. Global selection needs all candidates tied at the size cutoff.

**Decision: accepted.**


A `DirRecord` carries at most `top_files` top-N candidates, chosen by `(disk desc, inode asc)` within that directory. This preserves every possible global top-N winner under `(disk desc, final directory id asc, inode asc)`; cross-directory ties at the worker cutoff are still emitted. A separate over-threshold class must have an exact per-directory bound for `03` §4 Step G, or W1-B writes `docs/handoff/scr-W1-B/SCR-001.md` with the trade-off and implements the safest bounded behaviour. See G3 below and `04` §7 for the byte budget; the million-entry cap is not a candidate memory budget.

Plan locations: `04` §2.2; W1-B notes; W2-A selection. Affected tasks: W1-B, W2-A/B.

### Measured du oracle

SCR-006’s target-platform claim now has a measurement. The directory-inclusive metric remains normative across differing coreutils behaviour.

**Decision: accepted.**


In a `rockylinux:8` container, `du (GNU coreutils) 8.30` with `--apparent-size` was measured to count directory sizes; coreutils 9.2 and later do not. SCR-006 stands: probe local behaviour and normalize using measured directory/non-regular inode sizes when needed, never a version-string branch.

Plan locations: SCR-006 above; `09` §3. Affected tasks: W0-C, W1-B, W2-A/C.

### Filesystem-dependent test numbers

A `cargo test` run on overlayfs in a `rockylinux:8` container exposed literal allocation and directory-size assumptions. Such literals test the host filesystem rather than the contract.

**Decision: accepted.**


Measure allocated sizes (`st_blocks`), directory sizes and `du` figures; do not assert filesystem-dependent literals. Assert the `03` §6 worked example exactly only in the in-memory reference test and privileged loop-XFS test.

Plan locations: `09` §1; W1-B tests/acceptance; W2-A golden tests. Affected tasks: All fixture-test owners, especially W0-C, W1-B/C/E, W2-A/C.

### CLI surface tests

Surface tests that require an unimplemented-command error fail as implementations land. Those tests should verify the stable command contract.

**Decision: accepted.**


Assert commands, flags, help, parsing and exit-code mapping only, never that a command remains unimplemented.

Plan locations: `09` §1; W1-A SCR-002. Affected tasks: W0-A, W1-A, W2-C and later CLI owners.

### G2 — the gate factory returns a handle

**Decision: accepted.**

`gate_from_config` currently returns
`Result<Arc<dyn Gate>, ConfigIssue>`, so a caller cannot read statistics or stop the controller.
Change it to exactly:

```rust
pub struct ScanGate { /* private fields */ }
impl ScanGate {
    /// The gate to pass to the walker and the bulk pipeline.
    pub fn gate(&self) -> Arc<dyn Gate>;
    /// Statistics so far.
    pub fn stats(&self) -> GateStats;
    /// Stop the controller thread (joined within one interval) and return the final statistics.
    pub fn stop(self) -> GateStats;
}
pub fn gate_from_config(
    profile: &str,
    cfg: &ThrottleConfig,
    leaves: &[BlockDevInfo],
    sampler: StatSampler,
) -> Result<ScanGate, ConfigIssue>;
```

`GateStats` is `tidemark_core::scan::GateStats`, unchanged. For an adaptive gate the statistics are
the controller's. For a fixed-rate gate: `mean_rate` and `min_rate` are the fixed rate,
`throttled_ms` is the measured blocked time, `decreases` and `increases` are 0. For profile `max`
(no throttling): every field is 0, and the documentation says that a rate of 0 together with
profile `max` means "unthrottled". Dropping a `ScanGate` without calling `stop` also stops the
controller. Acquisitions already waiting when `stop` is called complete at the last rate.

Plan locations: `04` §6.2; W1-D; W2-C.

### G7 — exact token accounting

**Decision: accepted.**

The token counters are `f64` values that accumulate for the
life of the gate; at 2^53 adding one unit no longer changes them, so acquisitions stop being
charged. Replace the accumulated `f64` counters with exact integer arithmetic (for example
nano-tokens in `u128`, or `u64` with checked arithmetic and a rebasing step that preserves the
targets of waiters). Rates stay `f64` in the public API. No counter may wrap or lose precision
for any legal rate and any run length. Add regressions seeded near 2^53 units, with and without
outstanding waiters, asserting that each acquisition is still charged.

Plan locations: `04` §6.2; W1-D.

### G3 — bound the notable candidates per directory, exactly

**Decision: accepted.**

Since the tie fix, a directory
record keeps every file tied at the worker's size cutoff, which can be a million candidates
for one directory of equal-sized files. The global selection (`03` §4 Step G) takes `top_files`
files in the order `(disk desc, final directory id asc, inode asc)`. Inside one directory the
directory id is the same for all files, so one directory can contribute at most `top_files`
winners and they are its first `top_files` files by `(disk desc, inode asc)`. Therefore: a
`DirRecord` carries at most `top_files` top-N candidates, chosen by `(disk desc, inode asc)`
within that directory. This loses nothing. Keep the cross-directory behaviour (ties at the
worker's cutoff are still emitted). If files at or above `big_file_threshold` form a separate
class of candidates, bound that class per directory in a way that is also exact for the
normative selection, or, if it cannot be made exact, write
`docs/handoff/scr-W1-B/SCR-001.md` with the trade-off and implement the safest bounded behaviour.
State the resulting worst-case bytes held by the walker as a formula of `threads`, the output
channel capacity, `top_files` and the name length. Add a test with several wide directories of
equal-sized files and a slow consumer that asserts the per-record candidate count never
exceeds `top_files` and that the global winners are all present.
In addition, for the plan: the scan orchestrator (W2-C) sizes the walker's output channel from
the scan memory budget, so that `capacity * top_files * (bytes per candidate)` stays inside the
share of the budget given to queued records; `04` §7 must state the formula.

Plan locations: `04` §2.2/§7; W1-B; W2-C.

### G4 — never lose a mount boundary

**Decision: accepted.**

`parse_mountinfo` now skips a malformed line and returns
the rest, so a line with a readable mount point but a broken later field disappears from the
table, and the walker's "mount table unavailable" safety check does not trigger because the
result is `Ok`. Decide per line: if the mount id, parent id, device and mount point can be
decoded, keep the record as a boundary even when later fields are malformed (set those fields
to empty and log the line with `tracing::warn!`). If the mount point of any line cannot be
decoded, return `Err` for the whole table, so callers fail closed. Valid nsfs records are
unaffected. In the walker, keep the rule that when mount discovery returns `Err` and the kernel
reports no mount ids, the scan refuses to run. Add a regression: a table with a line whose final
field is missing still yields that line's mount point from `submounts_under`; a table with an
undecodable mount point returns `Err`.

Plan locations: `04` §2.2; W1-A/B.

### G5 — do not let SQLite touch a corrupt database before recovery

**Decision: accepted.**

When `timeseries.db`
holds garbage, `Catalog::open` (and `TimeSeries::open`) hands it to SQLite, which returns "not a
database" and deletes the `-wal` and `-shm` sidecars; `recover_catalog` afterwards can preserve
only the main file. Add a preflight used by every open of an existing `timeseries.db`: read the
first 16 bytes of the file yourself; if the file exists, is not empty, and does not start with
the SQLite header string `SQLite format 3\0`, return the corruption error **without** opening it
with SQLite. A missing or empty file is created as today. A file with a valid header is opened
normally, and SQLite's own WAL recovery applies. Reading Tidemark's own state file this way is
allowed; the "never read file contents" rule is about scanned files. Replace the test that
recreates the sidecars after a failed open with one that runs the real sequence: garbage in the
database and both sidecars present, ordinary open fails, then `recover_catalog` succeeds and the
original bytes of all three files exist under the recovery names. Keep the test for a valid
header with a damaged page behaving as SQLite decides.
The daemon and `snapshots rebuild-catalog` call `recover_catalog` after an open reports
corruption, with all connections closed, and log the recovery file names (W2-C, W6-A).

Plan locations: `05` §4; W1-E; W2-C; W6-A.

### G6 — `xtask vm-test` must pass test parameters and show output

**Decision: accepted.**

1. Add a repeatable option `--env NAME=VALUE`. `NAME` must match `TIDEMARK_[A-Z0-9_]+`; anything
   else is rejected before any connection is made. The pairs are passed into the remote test
   process through `ssh` and `sudo` next to `PATH` (`sudo env "PATH=$PATH" NAME=VALUE ... cargo
   test ...`), with the value quoted so that spaces and shell metacharacters cannot break out of
   the command.
2. Add `--nocapture`, which appends `--nocapture` to the test binary arguments, so diagnostic
   output of passing tests (for example the bulkstat v5 probe result) is visible.
3. Unit-test the command construction: accepted and rejected names, a value containing a space,
   a quote and a semicolon, and the position of the arguments.
4. In `xtask/README.md`, document how to provision a disposable md RAID array inside the VM from
   loop devices (the commands, run in the VM only, never on a development host) and how to pass
   it to the suite with `--env TIDEMARK_W1A_MD_DEVICE=/dev/mdX`; list the findings the Wave 1 gate
   asks the operator to record (bulkstat v5 at run time, the `bs_blocks` unit, the default I/O
   scheduler) and the command that shows each.
Do not try to reach a host from this sandbox.

Plan locations: `09` §5; W0-C xtask notes.

### SCR-007 — One display-escaping helper (G8)

**Decision: accepted.**

The
escaping of control characters, invalid bytes and backslashes exists twice. Add to
`crates/tidemark-core/src/path.rs`:

```rust
/// Escape bytes for terminal display: printable Unicode is kept; control characters, invalid
/// UTF-8 bytes and backslashes are written as uppercase `\xNN`.
pub fn display_lossy_bytes(bytes: &[u8]) -> String;
```

with exactly the behaviour `RawPath::display_lossy` has today; make `RawPath::display_lossy` and
`RawName::display_lossy` delegate to it, and use it in `crates/tidemark-fs/src/mount.rs` instead
of the copy there. This is the only change allowed in `tidemark-core`; do not change any other
core item or any existing behaviour. Existing tests must pass unchanged; add one that the three
entry points give identical output for a table of inputs.

Plan locations: W0-B path table; W1-A; SCR-004.

### Wave 2 readiness notes

**Decision: accepted.**

The bulk pipeline (W2-B) charges the `Gate` before
each call to the bulkstat iterator, with a conservative per-batch cost, not after the I/O with
the number of records returned; the first call also covers the geometry read. `BulkInode`
`disk_bytes` is already in bytes and must not be multiplied again. The orchestrator (W2-C), not
the bulk pipeline, resolves leaf devices and builds the gate. The CLI canonicalizes the original
path before it becomes `ScanRequest.root`.

Plan locations: `04` §5–7; W2-B/C.

---

## Wave 2 decisions

These final coordinator decisions supersede the pending decisions in `docs/handoff/scr-W2-A/SCR-001.md`, `scr-W2-C/SCR-001.md` and `scr-W2-C/SCR-002.md`. Implementation precision below follows `docs/handoff/W2-integ.md`.

### SCR-008 — Per-directory error totals (D1)

Bounded error detail samples cannot supply uncapped directory totals.

**Decision: accepted; approved change to `tidemark-core`.**

Add exactly one field directly after `errors` in each of `tidemark_core::scan::DirRecord` and `NsRecord`:

```rust
/// All errors attributed directly to this directory, whether or not a detail row was kept in
/// `errors` (which is a bounded sample).
pub errors_total: u32,
```

The walker and bulk namespace phase set it for every directory with checked conversion; more than `u32::MAX` directory errors returns `WalkError::Limit`. Aggregation uses `errors_total`, never `errors.len()`, for `DirRow.errors`, subtree totals and the summary. The sum over records must equal `WalkSummary.errors`. The orchestrator checks both modes, logs a warning on disagreement and marks degraded without replacing the record total. Bulk privately carries the namespace summary, without extending `BulkStats` for it. No other core change is authorized by this decision.

Plan locations: `04` §1/§3; W2-C sequence. Affected tasks: W1-B, W2-A/B/C.

### Bulk interface (D2)

**Decision: accepted in this form.**

```rust
pub struct BulkProgress { pub phase: &'static str, pub dirs: u64, pub entries: u64,
                          pub inodes: u64, pub errors: u64, pub races: u64 }
pub fn run_bulk(p: &BulkParams, mount_fd: BorrowedFd<'_>, geo: &XfsGeometry, gate: &dyn Gate,
                cancel: &AtomicBool, progress: &(dyn Fn(&BulkProgress) + Sync),
                agg: Aggregator) -> Result<(Prepared, BulkStats), BulkError>;
```

`BulkStats` gains `pub noatime_fallbacks: u64` from the namespace phase. `BulkError::BeforePhase2(String)` means a failure before the first phase-two bulkstat call has been issued, when nothing consumed prevents redoing by walk. Namespace collection, sorting-worker creation and geometry validation failures qualify. In auto mode only `Unsupported` or `BeforePhase2` falls back to walk with a **fresh** aggregator, updated mode and recorded `mode_reason`; other bulk errors fail. Cancellation takes precedence, returns `Cancelled` and produces no snapshot. `--keep-partial` is walk-only and `scan --help` says so. Progress is called at most every two seconds, sharing a cadence across phases including waits in collection, sorting and name resolution.

Plan locations: `04` §3/§8; `08` §1; W2-B interface and W2-C sequence. Affected tasks: W2-B/C.

### Memory budget (D3)

**Decision: accepted with modifications.**

**Replaced by R2-1 below.** The earlier estimate-only decision no longer excludes a record byte bound.

Plan locations: `04` §7; W2-C sequence. Affected tasks: W1-B, W2-C, W6-A.

### Snapshot id collision at publication (D4)

**Decision: accepted with modifications.**

The host-wide scan lock permits one scan at a time. Writer-creation collisions retain the existing fresh-id retries. A collision from `SnapshotWriter::finish` fails the scan with typed `ScanError::PublicationCollision`, leaves the existing snapshot untouched and is not retried. No store interface change is authorized. This qualifies SCR-004's earlier retry wording: retries apply at writer creation, not publication.

Plan locations: W2-C sequence steps 3/6/10. Affected tasks: W1-E, W2-C.

### `snapshots prune` (D5)

**Decision: accepted.**

Non-dry-run prune opens a writable catalog, deletes the files returned by the retention plan, then calls `Catalog::rebuild_from_files` globally from remaining snapshot files, removing deleted snapshot rows. `--dry-run` only reports proposed deletions. `snapshots rebuild-catalog --root PATH` is refused; omit `--root` for global rebuilding (R2-5 replaces the earlier scoped-command wording). No store interface change.

Plan locations: `05` §7; `08` §1; W2-C CLI. Affected tasks: W2-C, later catalog callers.

## Wave 5 requests

- `docs/handoff/scr-W5-C/SCR-001.md` and `SCR-002.md` are **closed without a decision** because the PostgreSQL collector was removed (ADR-019).
- `docs/handoff/scr-W5-D/SCR-1.md` is **closed by R2-10** below; W5-D-fix supplies the decided integration surface.


## Waves 2 to 5 review decisions

These coordinator decisions supersede pending labels in the listed requests and older conflicting plan text. Fix notes are the source for implemented details; their local checks are evidence, not independent verification. `W3-C-finish.md` is missing: its task-specific signatures remain **see the code**; the common accessors are established by STORE-seams. W4-A-fix adds no contract change. ADR-018 root-only access and ADR-019 PostgreSQL deferral remain in force.

### R2-1 — Walker record bound (replaces D3)

Candidate-only queue estimates did not bound link, skip or error storage. A wide directory could exceed the assigned record pool.

**Decision: accepted as specified below.**

`WalkConfig.max_record_bytes: u64` bounds each retained directory record (0 disables the bound only for explicit fixtures). Exceeding it returns `WalkError::Limit(String)` naming the directory, entry count and bound; no record is truncated. Charge before allocation for links, skips, sampled errors, namespace children and candidates, including vector growth, names/paths, padding and candidate heaps.

Let `Q = scan.memory_budget / 4` and `T = threads`. Refuse before starting if `Q < T * 16 MiB`. Compute `C = clamp(floor(Q / 16 MiB) - T, 1, 64)` and `max_record_bytes = floor(Q / (T + C))`. Capacity reaches 1 before the record bound drops below 16 MiB. Admission also checks aggregator estimates and worker buffers/heaps against the remaining budget using wide arithmetic, including bulk-to-walk fallback. Bulk's namespace channel has capacity 2, so bulk admission refuses derived `C < 2`. CLI live walks use the same derived record bound. This replaces D3: aggregate storage remains estimated, while records are explicitly bounded; the service cgroup supplies the hard whole-process limit.

Source notes: W2-C-fix (under docs/handoff/).

Requests closed: scr-W2-C/SCR-002 (memory part; D4 retains collision policy).

Plan locations: 04 §2.2/§7; W2-C. Affected tasks: W1-B, W2-A/B/C, W6-A.

### R2-2 — Checked directory read

A directory can be replaced or mounted over after namespace discovery. A name lookup must check the opened identity before reading.

**Decision: accepted as specified below.**

```rust
pub fn read_dir_entries_checked(root: &RawPath, dev: DevId, path: &RawPath,
    expect_ino: Option<Ino>, gate: &dyn Gate) -> io::Result<Vec<(RawName, Ino, FileKind)>>;
```

`tidemark_fs::walk::read_dir_entries_checked` opens each component relative to no-follow directory handles with O_NOATIME (EPERM fallback). Gated descriptor statx checks device and available mount ID against the opened root before enumeration, and the final inode against the expected phase-1 identity. Outside-root paths and identity/device/mount mismatches return `io::ErrorKind::CrossesDevices`; other open errors retain their I/O error. Without kernel mount IDs, a same-device bind preserving the expected inode cannot be detected. Bulk phase 3 and `links --show-paths` use this helper, never the unchecked read. Bulk failures leave names unresolved and count races; links render missing/replaced directories as `(gone)` and propagate other I/O failures. Apply original excludes and skipped-mount paths before choosing an alias.

Source notes: W2-B-fix; W3-B-finish (under docs/handoff/). The full checked-read signature was transcribed from crates/tidemark-fs/src/walk/getdents.rs.

Requests closed: No separate SCR; W2-B review and W3-B-finish.

Plan locations: 04 §3.3; 08 §1 links; W2-B/W3-B. Affected tasks: W2-B, W3-B, W6-A, W7-C.

### R2-3 — Aggregator bounds

Bulk candidate ingestion retained all single-link files. Sparse discovery IDs also made full node slots costly.

**Decision: accepted as specified below.**

Notable candidates are bounded during ingestion in both modes. Let D be candidate-bearing directories and M=`max_notable`: retain fewer than M rows above the cutoff and at most D*M at the cutoff, plus one transient insertion; below-threshold ties tighten to D*min(`top_files`, M). After final directory numbering, lazily drain into a selection capped at M in `(disk desc, final directory id asc, inode asc)` order. This is exact but does not promise O(M) memory independent of D.

The fixed fallback directory limit is `MAX_DIRS = 16_000_000`; exceeding it returns `AggError::Capacity`. Node payload is 232 bytes, sparse `Option<Box<Node>>` slots 8 bytes, or 240 bytes per populated directory before names, children, allocator overhead and vector slack. The 200-byte target was not reached. Linked scans additionally need temporary 32-byte union/stamp scratch slots plus numbering vectors; this ceiling is not a guarantee of fitting the configured budget.

Source notes: W2-A-fix (under docs/handoff/).

Requests closed: scr-W2-A/SCR-001 already closed by SCR-008; error totals remain unchanged.

Plan locations: 04 §7; W2-A. Affected tasks: W2-A/B/C, W6-A, W7-B.

### R2-4 — Store access and cancellable publication

Private reader state blocked streamed SQL queries. A pre-finish cancellation check could not observe cancellation during index construction and sync.

**Decision: accepted as specified below.**

```rust
impl Snapshot {
    pub(crate) fn conn(&self) -> &rusqlite::Connection;
    pub(crate) fn file(&self) -> &std::path::Path;
}
impl SnapshotWriter {
    pub fn finish_with(self, final_meta: &SnapshotMeta,
        cancelled: &dyn Fn() -> bool) -> Result<PathBuf, StoreError>;
}
```

The internal accessors support streaming bound read-only queries and immutable/read-only ATTACH; detach attached databases on every exit. `finish` delegates with an always-false callback. `finish_with` checks cancellation before indexes, after each of seven indexes, before metadata, and after close/file sync immediately before non-replacing rename. `StoreError::Cancelled` removes unpublished temporary output; the orchestrator maps it to `ScanError::Cancelled` and records cancellation without a published file. Publication cancellation discards even a keep-partial candidate. No callback runs after rename; cancellation is observed at checkpoints, not atomically with the flag and rename.

Source notes: STORE-seams; W3-A-finish; W3-B-finish (under docs/handoff/).

Requests closed: scr-W3-A/SCR-1; scr-W3-B/SCR-001; scr-W3-C/SCR-001; publication seam from W2-C review.

Plan locations: 05 §2/§3/§5; W2-C. Affected tasks: W1-E, W2-C, W3-A/B/C, W6-A.

### R2-5 — Prune and global catalog rebuild

Deleting files without reconciling the catalog left stale query entries. A scoped global rebuild would discard unrelated roots’ historical rows.

**Decision: accepted as specified below.**

Prune deletes retention-selected files and globally rebuilds the catalog from surviving snapshots; dry-run only reports deletions. Automatic retention and emergency pruning reconcile the catalog too; expired partial/failed artifacts are considered before protected complete snapshots. `snapshots rebuild-catalog --root PATH` is refused before recovery or mutation: omit `--root` for an explicit global rebuild. This follows W2-C-fix more precisely than the older D5 text, because the global store rebuild cannot preserve another root's historical failed/running rows.

Source notes: W2-C-fix (under docs/handoff/).

Requests closed: No separate SCR; W2-C review, replaces D5 scoped wording.

Plan locations: 05 §7/§8; 08 §1; W2-C. Affected tasks: W2-C, W6-A, W7-D.

### R2-6 — Diff of removed paths

Realpath fails when a queried snapshot directory has been deleted. Resolving unresolved parent components would make ancestor fallback ambiguous.

**Decision: accepted as specified below.**

`diff` first canonicalizes the original live path. On ENOENT, it canonicalizes the longest existing ancestor and appends the unresolved byte names. Refuse `..` in the unresolved suffix; existing symlink/parent semantics are handled by realpath before lexical normalization. This exception applies to diff so removed snapshot paths remain queryable.

Source notes: W3-A-finish (under docs/handoff/).

Requests closed: scr-W3-A/SCR-2.

Plan locations: 08 §1.1; W3-A. Affected tasks: W3-A, W7-A/D.

### R2-7 — Deployment confinement and compensation

The installed config path was lost before dispatch. Runtime service and file publication failures cannot satisfy an all-or-nothing guarantee.

**Decision: accepted as specified below.**

The CLI `Context` carries `pub config_path: PathBuf`; global `-c` names the installed destination. `deploy stage --from` loads and validates its source independently, even when the installed config is missing or invalid. Blocking pre-flight failures change nothing. After pre-flight, publication/service failures trigger best-effort restoration of previous config, marker and staged-record bytes, removal of new backup files, and independent attempts to restore enabled/active service state. Report initiating and compensation errors with instructions to retry stage or unstage. This is not a transaction: downtime, timestamps and newly created empty directories may remain.

Local filesystem eligibility is exactly `xfs`, `ext2`, `ext3`, `ext4`, `btrfs`, `tmpfs`, `zfs`, `f2fs`, `jfs`, `reiserfs`; unknown types block. ADR-018 requires destination writability/searchability and required ownership, with no running-as-root check. Production files are root:root, 0600; state directories 0700.

Stage, unstage and firstboot refuse unsafe `state_dir` before mutations: `/`, nonabsolute paths, `..` components, symlink/nondirectory ancestry, mount points, and paths equal to or containing tracked roots/sampler/watch mounts (including existing roots' canonical locations). Also refuse paths equal to or containing `/etc`, `/usr`, `/var`, `/var/lib`, `/run`, `/home`, `/root`, `/boot`, `/dev`, `/proc`, `/sys`, `/tmp`. This is containment by state_dir: `/var/lib/tidemark` and private descendants of `/tmp` are eligible. Unsafe/mounted deploy subdirectories, symlink/nonregular artifacts and malformed/path-mismatched markers block too. Existing unmarked production directories must already have 0700 root:root; change ownership/mode only for new directories or a valid marker identifying that directory. Marker-backed unstage/firstboot recover tracked roots from the staged config; path-only calls without a marker protect default roots, and CLI unstage also checks loaded roots.

Every operation under state_dir uses no-follow directory handles: component-relative opens, regular-file reads, exclusive temporary writes, renameat/unlinkat and descriptor fsync. Retain handles through service actions and compensation; the installed config is also inspected/read through a verified parent and O_NOFOLLOW.

Source notes: W4-B-fix (under docs/handoff/).

Requests closed: scr-W4-B/SCR-001, SCR-002, SCR-003.

Plan locations: 08 §1/§5; 11 P3/P5; ADR-016; W4-B. Affected tasks: W4-B, W6-A/C, W7-C/D.

### R2-8 — Watcher foreground events and directional renames

Aggregates cannot reconstruct individual event attribution. Directional fanotify records lack a reliable pairing cookie.

**Decision: accepted as specified below.**

```rust
pub struct WatchEvent { pub at: i64, pub path: RawPath, pub delta: i64, pub attribution: Attribution }
impl Watcher {
    pub fn enable_events(&self);
    pub fn drain_events(&self, limit: usize) -> Vec<WatchEvent>;
}
```

Only the foreground consumer enables the 1,000-event buffer; it drops oldest events and counts drops. Daemon construction leaves it absent and uses completed-minute drains. Foreground output shares a 50-lines/s limit across mounts, reports omissions, and emits JSON lines with schema `tidemark.watch.v1` and path/path_b64. MOVED_FROM/MOVED_TO are independent directional events with signed deltas; v1 uses no FAN_RENAME and reports no from-to pairs.

Retry EMFILE/ENFILE/ENOMEM/ENOBUFS with shutdown-aware exponential 50–800 ms waits, retaining the group and marking affected/recovery minutes lossy. Fatal transport failures persist as `WatchMode::Failed`; foreground CLI exits rather than appearing alive. `WatchStats` adds `dropped_events: u64`, `recovering: bool`, `read_errors: u64`. Clear unsafe path caches after loss; clock rollback discards affected ring history and marks the new minute lossy.

Source notes: W5-A-fix (under docs/handoff/).

Requests closed: scr-W5-A/SCR-001, SCR-002.

Plan locations: 07 §3; 08 §1; W5-A. Affected tasks: W5-A, W6-A, W7-C/D.

### R2-9 — Process identity and procfs charging

PID reuse between enumeration and attribution can assign an open inode to another process. Procfs metadata reads are not disk scan I/O.

**Decision: accepted as specified below.**

`OpenFile.starttime: u64` is the process identity validated around enumeration. Compare it with later `Attribution.starttime`; mismatch produces comm `?` and no unit for both phantom and writers. `/proc` reads do no disk I/O and are not charged to Gate; O_NOATIME does not apply. Existing bounded scanner caps remain. Writer reports use the actual baseline-to-latest timestamp span, not the requested window; positive growth with zero measured span is omitted.

Source notes: W5-B-fix (under docs/handoff/).

Requests closed: scr-W5-B/SCR-001 (identity accepted, procfs Gate proposal rejected).

Plan locations: 07 §4; W5-B. Affected tasks: W1-D, W5-B, W6-A.

### R2-10 — Rule-scoped alert source and unavailable measurements

Fixed aggregate windows could not evaluate arbitrary configured rule windows or automatic cadence. Missing measurements could appear as healthy zeros or resolve existing alerts.

**Decision: accepted as specified below.**

```rust
pub trait AlertSource {
    fn fs_sample(&self, mount: &str) -> Option<FsSample>;
    fn fs_rate(&self, mount: &str, window: Duration) -> Option<f64>;
    fn hot_dirs(&self, mount: &str, window: Duration) -> Option<Vec<HotEntry>>;
    fn watch_overflows(&self, mount: &str, window: Duration) -> Option<u64>;
    fn phantom_bytes(&self, mount: &str) -> Option<u64>;
    fn lvm(&self) -> Option<Vec<LvmNow>>;
    fn dir_growth(&self, root: &str, depth: u32) -> Option<Vec<(RawPath, i64)>>;
    fn scan_age(&self, root: &str) -> Option<ScanAge>;
}
pub struct ScanAge { pub last_complete_at: Option<i64>, pub effective_cadence: Option<Duration> }
pub struct AlertInputs<'a> { pub source: &'a dyn AlertSource, pub state_space_refused: bool }
pub struct AlertDiagnostic { pub rule: String, pub reason: String }
impl AlertEngine { pub fn diagnostics(&self) -> &[AlertDiagnostic]; }
```

Query the source per rule and exact Duration window; dir_growth compares snapshots and has no window. Inventories other than watcher hot top lists must be complete and bounded (superseded for watcher inputs by R3-1): `Some(empty)` confirms absence, `None` means unavailable. A rule without data is not evaluable: preserve state, values and timers without starting, resolving or renotifying; pause continuity and restart pending/clearing evidence on the next known observation. Show diagnostic reasons in alerts and `NowInputs.unevaluable`, propagated through `NowReport.degraded`. Unknown thin-pool percentages cannot resolve an active alert. Healthy unseen subjects allocate no state; the private 10,000-pair cap reports admission failures and protects unavailable active states.

`older_than = "auto"` is twice effective cadence; windowed schedules supply at least 24h cadence, non-windowed short cadences remain valid. Unknown cadence is diagnostic; a known root with no complete scan breaches staleness. Measured NowInputs sections use Option: None is “not measured”, Some(empty) is measured empty. Preserve the frozen core report via degraded reasons and never display an unavailable phantom placeholder as measured zero.

`WebhookDeliver::new(url, host)` has two arguments; there is no separate webhook redaction rule or config key in v1. Preserve escaped subjects and the documented fields/text. Only 2xx succeeds; redirects are disabled and failed deliveries, with the existing bounded queue, timeout and retries.

Source notes: W5-D-fix (under docs/handoff/). Full AlertSource signatures were transcribed from crates/tidemark-live/src/alerts/mod.rs because the note describes, rather than enumerates, its eight methods.

Requests closed: scr-W5-D/SCR-1 (accepted source/availability; no separate redaction).

Plan locations: 07 §6/§7; W5-D; W6-A/B. Affected tasks: W5-D, W6-A/B, W7-C/D.


---

## Gate review and Wave 6 decisions

These coordinator decisions supersede conflicting earlier wording, especially R2-10’s complete-inventory/arbitrary-window requirement for watcher-backed inputs. All eighteen requested handoff notes exist. Sources: W45-gate (G3/G4/G6 and LVM/process gaps), W23-gate (B1/S7/S8/N1), W4-B-fix2/fix3 (bytes and stage), W5-A-fix2 (dirty coverage), W23-fix-gate/queries/scan (cancellation/order/paging/provenance/timestamp), CI-fix-daemon-state (shared state policy), W6-prep/A1/A2a/A2b/B/A3/A3-fix/C (ownership and runtime seams), and TEST-e2e-scan/deploy/rules (evidence limits). Implementation precision follows those notes; follow-up proposals remain proposals, and reported tests are historical evidence, not new verification.

### R3-1 — Watcher-backed alert windows

Minute buckets discard the event timing needed for arbitrary alert durations. Truncated hot lists cannot provide a complete inventory.

**Decision: accepted.**

Watcher-backed rules (`hot_dir_growth` and `watch_lossy`) accept only exact whole-minute windows from 1 through 60 minutes. Other durations are not evaluable, with a diagnostic; never round or clamp. Keep 60 whole-minute buckets and a cumulative overflow total. Sample that total at each completed-minute boundary; a window overflow count is the difference between its two boundary samples, unavailable if either sample is missing. Hot data consists of per-minute top lists, not a complete inventory. If any included minute overflowed or was truncated at its cap, hot data for the window is unavailable. The ordinary hot API retains its four ranked-list windows.

Updated by 13 R4-7: LIVE-minutes implements CompletedMinute, drain_minutes and mount_identity with empty-minute and boundary-loss facts. The daemon exact-window/freshness consumer remains open; watcher-backed rules are still not evaluable.

Plan locations: 07 §3.3/§6.2; W5/W6/W7. Affected tasks: W5-A/D, W6-A1/A2b, W7-C/D.

### R3-2 — Dirty roots after loss

Overflow previously replaced nested-root hints with the mount root. A sibling scan must not clear another root’s loss coverage.

**Decision: accepted.**

`Watcher::take_dirty(&self, under: &RawPath) -> Vec<RawPath>` returns `under` itself after a loss, independently for every root, until that root has been taken. A parent, child or sibling take does not acknowledge another root. Loss acknowledgements use a FIFO capped at 1,024 roots; eviction conservatively makes a forgotten root dirty again, repeated loss resets acknowledgements, and clock rollback invalidates them. Ordinary ancestor dirty entries are retained for other callers; descendants are drained, sorted and deduplicated. The scheduler takes dirtiness only after that root's successful scan has published; failed/cancelled scans clear nothing.

Plan locations: 07 §3.3; W5/W6. Affected tasks: W5-A, W6-A2a/A2b.

### R3-3 — Deployment failure wording (G6)

The operator example promised change-free failure for every non-zero result. That contradicted the detailed compensation contract.

**Decision: accepted.**

Blocking pre-flight failures change nothing. After pre-flight, runtime failures use best-effort compensation; downtime, timestamps and newly created empty directories may remain. Replace “non-zero = nothing changed” with this distinction, consistent with R2-7 and W45 G6.

Plan locations: 08 §5; 11 P3. Affected tasks: W4-B, W7-D.

### R3-4 — Reduced daemon LVM view

The direct renderer needs fields absent from frozen LvmResult. Extending the wire payload is unnecessary for the decided reduced view.

**Decision: accepted.**

The daemon-served `lvm` report keeps `LvmResult { mounts: Vec<(String, LvmNow)>, vgs: Vec<VgInfo>, sampled_at: i64 }`. Compared with the direct command it omits segment type and the other/unmounted LV inventory and columns. No wire extension or new flag is added. `W6-A3.md` specifies that the CLI still reads 24-hour trends read-only from the store with the direct command's query/slope/window; missing segment/other-volume columns are omitted and a notice explains the reduced view.

Plan locations: 07 §1; 08 §2; W6/W7. Affected tasks: W6-A1/A3, W7-D.

### R3-5 — Process scan diagnostics

Writer history admission can lose information under its cap. Permission-count reporting has no v1 source contract.

**Decision: accepted.**

`ProcScanResult { at: i64, fds_seen: u64, writers_full: bool, writer_evictions: u64, duration: Duration }` describes a completed pass. `writers_full` says writer candidates exceeded the history cap; `writer_evictions` counts histories evicted for admission during that pass. The daemon publishes these diagnostics with `fds_seen` and persistence drops in component health. Permission-error counts are not reported in v1; do not fabricate a zero or extend frozen report DTOs to add them.

Plan locations: 07 §4; W5/W6. Affected tasks: W5-B, W6-A2b.

### R3-6 — Parse the loaded configuration bytes

Reopening a config pathname can parse bytes different from those installed or hashed. One bounded byte parser closes that seam.

**Decision: accepted.**

Add `Config::from_toml_bytes(bytes: &[u8]) -> Result<Config, CoreError>` in `tidemark-core`. It shares `Config::load`'s bounded UTF-8/TOML parsing, default merge and error precedence (`MAX_CONFIG_BYTES = 1 MiB`). The daemon hashes the bytes it actually loaded, and stage parses, installs and hashes one bounded no-follow read rather than reopening the pathname (`W4-B-fix2.md`).

Plan locations: 08 §3; W6. Affected tasks: W4-B, W6-A1.

### R3-7 — Scan cancellation

Low-rate reservations could block cancellation for minutes. Stopping the controller alone does not cancel waiting acquisitions.

**Decision: accepted.**

`ScanGate::cancel(&self)` is permanent and idempotent: blocked token waiters return within 250 ms, statistics remain readable, and callers check cancellation after each acquisition before metadata I/O. `ScanGate::stop(self) -> GateStats` remains unchanged: it stops the controller and ordinary waiters continue at the last rate. The `Gate` trait and `gate_from_config` are unchanged. Add `ScanGate::stop_sampling(&self)` to stop sampling while retaining the borrowed cancellation handle and stats. The orchestrator's scoped cancellation monitor polls every 50 ms.

```rust
pub fn read_dir_entries_checked_cancellable(
    root: &RawPath, dev: DevId, path: &RawPath, expect_ino: Option<Ino>,
    gate: &dyn Gate, cancel: &AtomicBool,
) -> io::Result<Vec<(RawName, Ino, FileKind)>>;
```

The existing checked reader remains unchanged; the additive reader checks cancellation through opens, retries, identity stats and refills. The 250 ms bound covers userspace gate waits, not an in-flight kernel syscall, index build, sync, recorder callback or retention operation. No universal kernel-I/O deadline follows from it (`W23-fix-gate.md`).

Plan locations: 04 §3.3/§6.2/§8; W2/W6. Affected tasks: W1-D, W2-B/C, W6-A2a.

### R3-8 — Orchestrator ownership and persistence

Taking the scan lock in both caller and orchestrator makes the scan Busy. Queue acceptance alone cannot prove durable recorder completion.

**Decision: accepted.**

`run_scan(cfg: &Config, req: &ScanRequest, recorder: &dyn ScanRecorder, cancel: &AtomicBool, progress: &(dyn Fn(&Progress) + Sync)) -> Result<ScanOutcome, ScanError>` owns both `state_dir/scan.lock` and the `ScanGate`. A caller must not take the lock first: doing so makes `run_scan` return `ScanError::Busy`. Scheduler admission serializes scheduled/API requests but delegates host locking, gate creation, space admission and retention to `run_scan`. `ScanRecorder` callbacks return persistence success/errors, not enqueue success; daemon start/finish callbacks wait for error-bearing writer acknowledgements. `W6-A2a.md` reports that its unit Flush acknowledgement cannot attribute a particular series persistence error; its proposed `DirSeriesAck` is an unresolved implementation seam, not a new decided API.

Plan locations: 04 §8; 05 §6; W2/W6. Affected tasks: W2-C, W6-A1/A2a.

### R3-9 — Completion order

Retention can delete the predecessor needed for growth. Post-return growth computation is therefore too late.

**Decision: accepted.**

Successful completion is ordered: `ScanRecorder::scan_finished(&self, e: &ScanEntry) -> Result<(), ScanError>`, then `ScanRecorder::dir_series(&self, root: &RawPath, scan_id: &str, ts: i64, rows: &[DirRow]) -> Result<(), ScanError>`, then retention. The previous snapshot remains available during both callbacks. The daemon computes directory growth inside `scan_finished`, before retention can delete the predecessor. Successful metric-cache refresh/publication precedes that root's dirty take. A growth list that may be truncated is unavailable (`None`), never measured empty or a complete inventory.

Plan locations: 05 §7; W2/W6. Affected tasks: W2-C, W6-A2a/B.

### R3-10 — Indexed notable paging and diff provenance

Repeated sorting made notable export paging expensive. Partial/degraded diffs also concealed their input coverage.

**Decision: accepted.**

Notable ncdu rows page by indexed `(dir_id, id > last)` key in ascending stored file ID, using the existing `files_dir` index's rowid suffix. Start at ID -1 so ID 0 is included; no schema/index change or per-page sort is needed. Directory order remains preorder. Diff provenance is a human From/To header containing snapshot ID, mode, age, status and degraded marker, plus the existing JSON `notice` field for incomplete-coverage and age warnings. The three-field JSON payload is unchanged; no new provenance fields (`W23-fix-queries.md`).

Plan locations: 05 §5.1/§10; W7. Affected tasks: W3-A/C, W7-A/B/D.

### R3-11 — Wave 6 task split

One W6-A task mixed lifecycle, scan scheduling, collectors and transport. The implementation notes divide those responsibilities into separate owners.

**Decision: accepted.**

W6-A is split into W6-A1 (lifecycle, state, sampler, LVM, ts-writer), W6-A2a (scheduler, scan control, post-scan, firstboot report), W6-A2b (watcher, process-scan and alerts), and W6-A3 (API and CLI live commands). The wave table and ownership lists below are authoritative; the collective W6-A section retains its structure. W6-prep’s old W6-A2 scope is divided between A2a and A2b.

Plan locations: W6 task table/ownership. Affected tasks: W6-A1/A2a/A2b/A3, W6-B/C.

### R3-12 — Local deployment state filesystem

The fake-host deployment test exposed stage accepting NFS state. Tracked roots already required a local filesystem; state now has the same eligibility rule.

**Decision: accepted.**

A failed state-filesystem eligibility check reports the configured state path and filesystem type, exits with status 5, and makes no host changes. This check applies only to `deploy stage`; `deploy unstage` and `deploy purge` remain available for removing an installation whose state directory is on an ineligible filesystem. It does not add a daemon or first-boot check.

The filesystem holding `general.state_dir`, or its longest existing ancestor when missing, must be allowlisted local: xfs, ext2, ext3, ext4, btrfs, tmpfs, zfs, f2fs, jfs, reiserfs; unknown types block. Use the blocking `state_filesystem` pre-flight result. W4-B-fix3’s proposed §5 wording is adopted verbatim.

Plan locations: 08 §5; 11 P0/P3. Affected tasks: W4-B, W6-C, W7-D.

### R3-13 — Capability-based O_NOATIME fallback

Uid 0 does not imply CAP_FOWNER. Capability-free root tests observed a real fallback on foreign-owned directories.

**Decision: accepted.**

`O_NOATIME` permission depends on `CAP_FOWNER` or directory ownership, not uid 0. Without that capability, opening a directory owned by another uid falls back on EPERM, charging the retry and counting `noatime_fallbacks`. Root without capabilities can fall back; the measured fallback count is the evidence (`TEST-rules.md`).

Plan locations: 04 §2.2; 12 risks; W7. Affected tasks: W1-B, W7-C/D.

### R3-14 — Live command and peer contracts

Own-uid authentication in earlier plan text allowed nonroot peers. CLI effective configuration and direct fallback behavior also needed explicit labels and boundaries.

**Decision: accepted.**

Every API method serves only peer uid 0, identified by kernel `SO_PEERCRED`, even if the daemon runs with another uid; all other peers receive `forbidden`. The workspace `rustix` dependency enables `net` and `termios`. `status --show-config` prints the CLI's effective configuration, explicitly labelled; the running daemon may retain an older config until reload. There is no `config.get` method in v1. With no daemon, `lvm`, `writers` and `phantom` fall back to direct mode with one stderr notice (including quiet mode); `now`, `hot`, `alerts` and `status` exit 4. Explicit direct mode bypasses the daemon. A connected daemon scan refuses threads, extra excludes and keep-partial options; its request carries only the frozen root/mode/throttle overrides.

Plan locations: 02 §5; 08 §1/§2; 12 ADR-018; W6/W7. Affected tasks: W6-A3, W7-C/D.

### R3-15 — Known limitations remain open

Local tests and compile-only evidence leave runtime and interface gaps. Recording them as decided fixes would overstate the implementation.

**Decision: record as open risks; no resolution decided.**

Record the limitations in `12`’s open-risk register: N1 timestamp before indexes/sync; duplicate path-included deployment helper; uncancellable LVM worker; current-run alert history; SQLite pathname leaf opens; no privileged runtime evidence; counting-gate blind spot; socket-pair deployment fixtures. No new signatures are authorized to close these risks. Additional unresolved producer/writer/storage seams from A2a/A2b are identified there separately.

Plan locations: 12 open risks; 11; W7 gates. Affected tasks: Store/FS/daemon owners, W7-B/C/D.

### R3-16 — Bulk apparent size follows the on-disk inode

On Rocky 8.8/8.10, freshly copied fixtures had zero bulk apparent sizes for regular files while disk usage and link metrics agreed with walk. The supplied Rocky 8.10 kernel 4.18.0-553.el8_10 experiment confirmed that XFS bulkstat `bs_size` follows the on-disk inode until data writeback; the decoder correctly copies it. Flushing the destination made all nine mandatory fixtures agree. Per-file fixture `sync_all` also made the 120,000-file fixture unnecessarily slow.

**Decision: accepted.**

In bulk mode regular-file `apparent` is the size XFS has recorded on disk. Data waiting for writeback can make it smaller than `stat`, down to zero for a new file or the last written-back size for a growing file (normally recent writes of about 30 seconds, without a guaranteed deadline). Disk usage, counts, link accounting and charged bytes are unaffected. Walk uses the in-memory size, exact at scan time. A quiescent tree means no changes during scans **and no data waiting for writeback**. Production bulk scans never call `sync`, `syncfs` or `fsync`, and never stat files to correct sizes; no schema or algorithm change follows.

Tests flush fixture filesystems after writing/copying and before comparing modes or apparent-size oracles. Materialization performs one `rustix::fs::syncfs` on the fixture root after the whole fixture is written, replacing per-file syncs. A privileged writeback test permits spontaneous writeback: unflushed bulk apparent size must be no larger than walk, disk bytes must agree, and flushed rows must agree after ordinary mode-only normalization.

Plan locations: 01 FR-3/NFR-1; 03 §2/§3 apparent definition/oracle/invariant 6; 04 mode table/output oracle, §5.1 and §10; 08 §1 scan modes; 09 §3/§5; 11 first-night mode choice; W2-B tests. Affected tasks: W0-C (fixture writer), W2-B/C, W7-B/D.


## First real runs and Wave 6 review decisions

Recorded 2026-10-12 from the first real Rocky runs, owner decisions and fix handoffs. All listed source notes exist. Exact implemented contracts below follow the notes where more precise than the brief; open producer/state seams are distinguished from implemented behavior. R4 decisions supersede earlier conflicting plan text.

### R4-1 — Retained daemon state directory

**Decision: accepted.**

The daemon addresses its retained verified state directory as `/proc/self/fd/<n>/.`, so no-follow directory opens address the directory rather than the final procfs symlink. Daemon tests bind storage as production does.

Sources: `W6-fix-daemon-scan.md`.

Plan locations: 08 daemon lifecycle; W6-A1/A2.

### R4-2 — Scan failures and scheduler recovery

**Decision: accepted.**

Log one WARN per actual failed scan (root, mode, source, attempt and retry time), and INFO on admitted start and successful completion with scan id. Scheduler health remains degraded for each failed root until that root completes successfully or leaves configuration; waiting, another root’s success and reload do not establish recovery. Scheduled retries are 30, 60, 120, 240, 480, 960, 1920, then 3600 seconds, capped there. Completion and accepted reload reset retry delay/attempts. Busy and cancellation are not failures and use fixed 30-second deferral. Operator requests bypass backoff. Requested roots outside configuration retain failure health until recovery.

Sources: `W6-fix-daemon-scan.md`.

Plan locations: 04 §8; W6-A2.

### R4-3 — Portable catalog paths

**Decision: accepted.**

`scans.file` stores raw bytes relative to the catalog state directory, retaining its SQL TEXT type. `record_start`, `record_finish` and `rebuild_from_files` remove dot components and strip the lexical state-directory prefix; outside paths, unanchored relative inputs and parent traversal are invalid. No symlink resolution occurs. `list`, `latest`, `nearest_before` and `get` join against the reader’s state directory. Existing legacy absolute files are returned unchanged; missing absolute files use the tail beginning at the last `snapshots` component if that file exists locally, otherwise remain unchanged. Rebuild repairs legacy rows to relative paths. `ScanEntry` and Catalog signatures are unchanged.

Sources: `STORE-catalog-paths.md`.

Plan locations: 05 §4.

### R4-4 — RPM construction and continuous integration

**Decision: accepted.**

The CLI manifest contains an empty `[package.metadata.generate-rpm]` table for external metadata. `cargo xtask rpm --native` builds on EL8; the changed-default test package requires `--native --version 0.1.1 --upgrade-fixture`. Native mode installs nothing, defaults offline and rejects foreign hosts/missing generator. `tools/check-rpm.sh` checks package identity, exact assets/modes/owners/config flags, scriptlets and extracted ELF binaries. `tools/el8-build-env.sh` prepares disposable EL8 build environments, pinning 8.8 repositories to the vault. GitHub Actions has a `ci` job and separate Rocky 8.8/8.10 `rpm` matrix, locked fetching, package/install verification, checksums and artifacts. ShellCheck runs on GitHub at default severity. Owner decision: push main to GitHub after each green local gate; this task does not push.

Sources: `PKG-rpm-metadata.md`, `CI-rpm.md`, `INSTALL-assert.md`.

Plan locations: 09 §9; W6-C.

### R4-5 — Service lifecycle and discriminating install assertions

**Decision: accepted.**

Unit values have no trailing comments; the existing normative unit remains unchanged. Install neither starts nor enables. Upgrade/reinstall uses guarded `try-restart`: restart a running daemon and leave a stopped daemon stopped, preserving enablement. Erase stops/disables and retains state. The install gate requires effective memory limit 2147483648, notify/root, NoNewPrivileges and exact capability names, and rejects installed-unit parser warnings. Active upgrade must change a nonzero PID and regain API readiness; stopped reinstall stays inactive. Edited config survives, and the changed-default fixture must produce root:root 0600 `.rpmnew` with its marker. Enable and prove active/enabled before erase, then assert inactive/disabled and no wants entry even as a dangling symlink; retained `.rpmsave` matches edited bytes and mode 0600. Require SELinux Enforcing unless an explicit permissive override labels SELinux acceptance untested. Post-reboot verification requires an external writer’s hot directory on restored disposable XFS; any post-mount restart is explicit.

Sources: `PKG-unit-fix.md`, `PKG-upgrade-restart.md`, `INSTALL-assert.md`.

Plan locations: 09 packaging gate; 11 rollout; W6-C.

### R4-6 — Live hot report and attribution

**Decision: accepted.**

`now.hot_dirs_5m` is the watcher’s ranked five-minute directory list including the current minute, bounded by writers row limit and preserving lossy flags; it does not establish exact alert windows. Missing/failed watchers are unmeasured; an empty running list is measured. `top_unit` is the unit with greatest sum of absolute byte deltas, ties to the lexicographically smaller name, using at most eight private space-saving candidates per row/minute (approximate beyond eight). Unit rows identify themselves; unattributed rows have None. A process gone before `/proc` lookup stays unattributed without retry/guessing. Persisted `HotMinute` has no `top_unit`. Multi-minute `files` sums per-minute distinct counts, not distinct files across the window.

Sources: `LIVE-hot.md`, `EDGE-cli.md`.

Plan locations: 07 §3.3/§7/§7a; 08 live CLI.

### R4-7 — Completed minutes and watcher identity

**Decision: accepted.**

`tidemark_live::watch::CompletedMinute` (Clone, Debug) has `started_at: i64`, `overflow_total_at_start: u64`, `overflow_total_at_end: u64`, `lossy: bool`, `truncated: bool`, `dirs: Vec<HotEntry>`, `persisted: Vec<HotMinute>`. `Watcher::drain_minutes(&self, top_k: usize) -> Vec<CompletedMinute>` shares the persistence-row cursor with legacy drains, while minute facts have an independent monotonic watermark. New drains exclude the startup partial minute, materialize idle minutes and remain bounded by 60 buckets; top_k is clamped to 1000 per scope, cuts mark truncation/loss. Retention gaps mark the first surviving minute lossy; rollback never replays emitted timestamps/rows and marks the replay interval lossy. Only actual queue overflow increments totals; contiguous boundaries chain. `Watcher::mount_identity(&self) -> std::io::Result<(u64, u32, u32)>` returns retained descriptor mount id, device major/minor from bounded fdinfo/fstat; injected watchers return Unsupported. Marks target the retained `/proc/self/fd/<fd>/.` directory.

The daemon consumer must retain at most 60 completed facts per watcher identity, discard on replacement and persist only `persisted`. Exact n-minute windows require 1 <= n <= 60, end E of latest completed minute, start S = E - n*60, and measured_at E (never refreshed artificially). Overflow differences need consistent nondecreasing exact S/E boundary totals; otherwise None. Known endpoints suffice even with interior loss. Hot requires n consecutive healthy, untruncated, overflow-free minutes; otherwise None. Sum raw-key rows with saturating counts/deltas, rank signed bytes descending then raw key ascending; healthy empty is Some(empty). A daemon union cap cut makes hot unavailable. Minute leaders cannot reconstruct exact cross-minute unit leadership. These consumer publications remain open.

`AttributionCache::lookup_watcher` may reuse PID/starttime validation for less than one second, separately from its 60-second attribution TTL. Older hits revalidate; failed/recycled identities invalidate and fresh loads recheck. Failed lookups are not cached. Exact process-scanner callers validate every lookup.

Sources: `LIVE-minutes.md`, `WATCH-perf.md`.

Plan locations: 07 §3/§4.3; W7 hardening.

### R4-8 — EL8 watcher CPU budget

**Decision: accepted.**

Owner decision: fd-mode watcher budget is 5% of one core per 1,000 close-write events/s, or 50 microseconds/event, replacing 1%. EL8 kernels 4.18.0-477 and -553 offer no measured FID mode. Optimised 8.10 guest measured 45.6 microseconds/event at 2,000/s (9.1% of one core) with host under load. Owner will measure real servers; this is no universal performance guarantee. The 2,000/s scale test threshold is 10% of one core, replacing 2%. Idle CPU and write-throughput impact remain separate gates.

Sources: `docs/handoff/WATCH-perf.md`; owner decision in PLAN-round4 brief.

Plan locations: 01 NFR-4; 07 §8; 12 risks; w5a privileged scale test.

### R4-9 — Exact configured mount presence

**Decision: accepted.**

A normalized sampler.mounts/watch.mounts path must exactly equal a mount point in the daemon namespace; identity is topmost mount id plus device major/minor. Absent entries produce no samples/persistence or watcher and report `not mounted`. Presence checks pick up arrivals (sampler at most ten seconds, watcher every second); identity changes release the old watcher before replacement. Open sampling descriptor identity is checked before statvfs. Covered scheduled and requested scan roots are refused before run_scan, following existing error/backoff handling; unrelated roots remain eligible. Firstboot is unhealthy for absent entries, but startup readiness is prompt.

Open: clearing in-memory filesystem rate series/removing stale samples across appearance/disappearance/remount; current removable missing-mount reasons in `now.degraded`; daemon consumption of retained watcher mark identity and the residual check-to-mark race. Service persistence deduplication reset is not a rate-ring reset. The proposed future state behavior in W6-mount-presence is not claimed implemented.

Sources: `W6-mount-presence.md`, `LIVE-minutes.md`.

Plan locations: 07 §1/§3; 04 §8; 12 open risks.

### R4-10 — Transactional configuration reload

**Decision: accepted.**

Invalid config.reload returns ok with `reloaded: false` and the error in warnings, keeping previous config/generation. Inventory/capacity/catalog/history/schedule preflight failures are contained the same way, without stopping scheduler. Reserve current configured roots transactionally within the 128-entry inventory, evict inactive manual history deterministically if needed, retire removed inactive roots and retain admitted/running roots until completion. Stale schedule generations cannot resurrect removed roots.

Sources: `docs/handoff/W6-mount-presence.md`, `docs/handoff/CORE-fix-1.md` B2.

Plan locations: 08 API config.reload; W6-A1/A2.

### R4-11 — Shutdown and pre-publication validation

**Decision: accepted.**

One shared monotonic shutdown deadline governs recorder queue/acknowledgement waits and startup-error cleanup; poll cancellation/deadline at most every 50 ms while preserving eight-second per-operation limits. Before admission work is uncommitted; interruption after admission reports persistence uncertain. Retain/join worker handles even after reporting deadline expiry; the ten-second overall bound is unchanged and arbitrary blocked kernel/SQLite operations remain a limitation. Parse exactly the bounded no-follow captured bytes that are hashed. Startup, reload and --check-config validate core/bounds, scheduler schedules/windows and alert predicates before publication. Runtime intervals (throttle sample, sampler, LVM interval/timeout, procscan, metrics, alert renotify/for/resolve_for and every schedules) are at most 366 days inclusive; historical retention horizons are exempt.

Sources: `docs/handoff/CORE-fix-1.md` B1, S6, S9 and F12.

Plan locations: 08 validation; W6 lifecycle.

### R4-12 — CLI scan ownership and presentation

**Decision: accepted.**

Scan falls back directly only when socket is absent/refuses, with an absolute connection deadline rechecked under scan lock. Other connection failures exit 4 before direct state mutation. Follow only the started scan id and read its persisted outcome; degraded, partial or unknown outcome exits 3. Never follow a replacement scan or automatically cancel another id. Root-only scan.cancel remains an explicit root operation. Tables use positive stdout TTY width or 100 otherwise, shortening cells in the middle while preserving escaped tokens; prose wraps at whitespace. Human absolute times are UTC with Z; JSON integers remain integers. now displays provided rate_15m in hourly units and provided one-hour ttf_secs without recomputation.

Open: `tidemark.lvm.v1` names both direct and reduced daemon shapes; scan.cancel cannot atomically condition on scan id under the frozen API.

Sources: `EDGE-cli.md`.

Plan locations: 08 CLI/methods/exit codes; 12 open risks.

### R4-13 — Bounded metrics selection and restart recovery

**Decision: accepted.**

Independent global directory rankings retain at most the configured series cap per family and count omitted eligible rows exactly, with deterministic raw root/path ties, rather than refusing large inventories before selection. Startup and root/depth/capacity reload reconcile a read-only latest-complete snapshot cache. Crash-left owned single-link regular staging files are flocked, truncated and reused through a collector-directory handle; hostile or live staging files are refused. Nonrepresentable metrics interval reports component error until a new generation.

HTTP removal is separately assigned ADR-020 work: do not adopt EDGE-metrics’ HTTP proposal here. Existing metrics.listen and alerts.webhook text is intentionally retained; see the PLAN-round4 handoff inventory.

Sources: `EDGE-metrics.md`.

Plan locations: W6-B; 09 metrics tests.

### R4-14 — Guest harness execution rules

**Decision: accepted.**

Harness-phase tests print a skip line unless their phase/RPM variables are supplied; explicitly requested tests require the explicit VM marker, never inferred. `cargo xtask vm-test --no-fail-fast` passes the option to Cargo before -- and rejects it for script suites. Systemd-launched test binaries are copied out of home to an executable location with restored labels. SSH is the first ssh on PATH, enabling wrappers. Loop/md fixtures do not survive reboot: external harness restores them before verify. Compile-only, printed skips and socket-denied early returns are not guest acceptance.

Sources: `TEST-vm-harness.md`.

Plan locations: 09 VM tests; 11 rollout.

### R4-15 — Measured Rocky platform facts

**Decision: accepted.**

First real runs on Rocky 8.8/8.10: bulkstat v5 works on both; statx supplies no mount id on either. fanotify fd mode works on both; filesystem marks work on 8.10, while 8.8 uses mount marks and cannot observe writes through another mount of that filesystem. On 8.10 the package installs/upgrades/erases and survives reboot under SELinux Enforcing without denials. Tidemark ships no SELinux policy and supports Enforcing, Permissive or Disabled; enforcing is a test posture, not a product prerequisite. Do not infer remaining VM/soak gates passed from these observations.

Sources: first-run evidence in PLAN-round4 brief; `docs/handoff/TEST-vm-harness.md`, `docs/handoff/WATCH-perf.md`.

Plan locations: 04 §5.1; 07 §3.1; 12 platform facts.

### R4-16 — Open work after the reviews

**Status: open risks; no behavior change decided.**

Open, not new accepted behavior: watcher-backed alert rules remain unevaluable until daemon publishes exact CompletedMinute windows (R4-7); W6-A2b alert reconfiguration continuity, component removal and cancellable process scan; mount rate-ring reset/current now.degraded (R4-9); one intermittent cancellation deadline miss on 8.8 in scan_lifecycle::interrupted_cancelled_and_failed_scans; firstboot report rewritten on every start with changed config; LVM schema/cancel-id gaps (R4-12).

Review findings without landed fix notes: core S1 writer rollover failure/supervision, S2 stale collector alert availability, S3 procscan published-lock contention, S4 LVM retry units/backpressure, S5 wall-clock operational deadlines, S7 late sampler generation resurrection, S8 internal alert capacity; edge F02 incomplete-request API deadline, F03 abandoned handler cancellation, F04 mutable API parent path operations. B1/B2/S6/S9 have CORE-fix-1; F05–F08 EDGE-cli; F09–F12 EDGE-metrics plus CORE-fix-1 validation; F13 INSTALL-assert. F01 has an EDGE-metrics fix note but HTTP removal/ADR-020 is separately assigned and not resolved by this plan task. Landed notes establish implementation evidence, not independent verification; unrestricted socket tests and guest gates remain required.

Sources: `docs/reviews/W6-core-review.md`, `docs/reviews/W6-edge-review.md` and the fix notes listed above.

Plan locations: 12 open risks; W7 gate.
