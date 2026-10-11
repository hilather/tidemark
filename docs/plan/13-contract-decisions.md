# 13 — W0-B1 contract decisions

The coordinator accepts every finding in `docs/reviews/W0-B1-review.md` and decides SCR-001–004 **MODIFY (accepted with modifications)** as specified below. This decision record takes precedence over the “Decision: Pending” lines in `docs/handoff/SCR-001.md` through `SCR-004.md`. These are the decided contracts for later tasks; the implementation changes are a separate task.

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
