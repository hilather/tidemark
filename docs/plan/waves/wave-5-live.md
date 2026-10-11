# Wave 5 — Live monitoring components

**Goal:** the L2 components as tested libraries with standalone CLI modes: the fanotify watcher, the `/proc` scanner (phantom files, open writers), and the rate/alert/`now` model. Wave 6 wires them into the daemon.

**Agents:** 3 in parallel. **Entry:** `wave-1-done`. Can run alongside Waves 2–4: the owned crates are disjoint.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W5-A | fanotify FFI and watcher | `tidemark-sys/src/fanotify.rs`, `tidemark-live/src/watch/**`, `tidemark-cli/src/cmd/watch.rs` | W1-D (attribution) |
| W5-B | Process scanner: phantom files and open writers | `tidemark-live/src/procscan.rs`, `tidemark-cli/src/cmd/{phantom,writers}.rs` | W1-D |
| W5-C | *(Deferred.)* | — | — |
| W5-D | Rates, alert engine, `now` model and renderer | `tidemark-live/src/{rates,now}.rs`, `tidemark-live/src/alerts/**` | W0 |

To keep the three tasks independent, **W5-D's inputs use only `tidemark-core::live` types plus its own rate types**. It never imports from W5-A/B. Wave 6 adapts component outputs into W5-D's input structs.

**Merge order:** W5-D → W5-B → W5-A.

---

## W5-A — fanotify FFI and watcher

**Read first:** `07-live-monitoring.md` §3 (all), `02-architecture.md` §8 (mount-namespace rule), `fanotify(7)`, `fanotify_init(2)` and `fanotify_mark(2)` on the VM.

### Interface

```rust
// tidemark_sys::fanotify
pub struct FanotifyCaps { pub available: bool, pub fd_mode: bool, pub report_fid: bool,
                          pub report_dfid_name: bool, pub mark_filesystem: bool, pub error: Option<String> }
pub fn probe(mount: &Path) -> io::Result<FanotifyCaps>;   // tries each init/mark variant; closes everything
pub enum InitMode { FdMode, Fid, DfidName }
pub struct Fanotify;   // owns the fd
impl Fanotify {
    pub fn init(mode: InitMode) -> io::Result<Fanotify>;                       // FAN_CLASS_NOTIF | FAN_CLOEXEC [| report flags]
    pub fn mark_mount(&self, path: &Path, mask: u64) -> io::Result<()>;
    pub fn mark_filesystem(&self, path: &Path, mask: u64) -> io::Result<()>;
    pub fn read<'b>(&self, buf: &'b mut [u8]) -> io::Result<EventIter<'b>>;    // EAGAIN → empty
    pub fn as_fd(&self) -> BorrowedFd<'_>;
}
pub enum RawEvent { Overflow,
                    Fd  { mask: u64, pid: i32, fd: OwnedFd },
                    Fid { mask: u64, pid: i32, fsid: [i32; 2], handle_type: i32, handle: Vec<u8>, name: Option<Vec<u8>> } }
pub fn open_by_handle(mount_fd: BorrowedFd<'_>, handle_type: i32, handle: &[u8]) -> io::Result<OwnedFd>; // O_PATH
pub mod mask { pub const CLOSE_WRITE: u64; pub const MODIFY: u64; pub const CREATE: u64; pub const DELETE: u64;
               pub const MOVED_FROM: u64; pub const MOVED_TO: u64; pub const ONDIR: u64; /* … */ }

// tidemark_live::watch
pub struct WatcherOptions { pub mount: RawPath, pub events: Vec<String>, pub hot_depth: usize,
                         pub exclude_prefixes: Vec<RawPath>, pub lru_entries: usize }
/// Display strings (status, now, firstboot): "fd/mount", "dfid-name/mount", "dfid-name/filesystem".
/// FID-only (FAN_REPORT_FID without DFID_NAME) is deliberately not used: without names, resolving
/// directory-entry events needs a handle→path lookup per event, so the watcher falls back to fd mode.
pub enum WatchMode { FdMount, DfidNameMount, DfidNameFilesystem }
pub enum HotScope { Dir, Unit, Comm }
pub struct WatchStats { pub mode: WatchMode, pub events_total: u64, pub overflows_total: u64,
                        pub lossy_minutes_15m: u32, pub dropped_keys: u64 }
pub struct Watcher;
impl Watcher {
    pub fn start(cfg: WatcherOptions, attribution: Arc<AttributionCache>) -> Result<Watcher, WatchError>; // spawns its thread
    pub fn mode(&self) -> WatchMode;
    pub fn hot(&self, window_min: u32, scope: HotScope, limit: usize) -> Vec<HotEntry>;
    pub fn drain_completed_minutes(&self, top_k: usize) -> Vec<HotMinute>;   // for timeseries; each minute once
    pub fn take_dirty(&self, under: &RawPath) -> Vec<RawPath>;
    pub fn stats(&self) -> WatchStats;
    pub fn stop(self);                                                         // joins within 1 s
}
pub fn namespace_ok() -> io::Result<bool>;   // /proc/self/ns/mnt == /proc/1/ns/mnt
```

### Details

- **FFI:** use `libc` for `fanotify_init`, `fanotify_mark` and `fanotify_event_metadata`. Define constants missing from `libc` (`FAN_REPORT_FID = 0x200`, `FAN_REPORT_DFID_NAME`, `FAN_MARK_FILESYSTEM = 0x100`, info record types) with comments citing the kernel UAPI header. Parse info records (`fanotify_event_info_header`, `fanotify_event_info_fid` + `file_handle` + optional NUL-terminated name) with bounds checks on every length. **Never trust `event_len`**: validate it against the remaining buffer.
- **`probe`:** for each `InitMode` from richest to poorest, try init, then mark (filesystem, then mount) on the given path with a harmless mask, then close. Record what worked; `EINVAL`/`EPERM` are expected outcomes, not errors.
- **Watcher thread:**
  - `poll()` on the fanotify fd plus an `eventfd` for shutdown.
  - Read into 64 KiB.
  - Handle events per `07` §3.2 (fd mode) or §3.4 (FID modes). **Close every event fd** (the `OwnedFd` drop handles it).
  - Ignore our own PID.
- **Rings:** `07` §3.3. 60 minute buckets, with three bounded maps per bucket and a `lossy` flag. A minute is "completed" when the wall clock passes its end. `drain_completed_minutes` hands each completed minute's top-K rows over exactly once.
- **Two mounts by default** (`/` and `/data`): the daemon starts one `Watcher` per mount, each with its own fanotify group and thread. Exclusions come from the generic `watch.exclude_prefixes` setting.
- **`start` refuses** (`WatchError::Namespace`) when `namespace_ok()` is false, and returns `WatchError::Unsupported` when fanotify isn't available.
- **`cmd/watch.rs`:** foreground mode prints `HH:MM:SS  +12.3 MiB  /path  (comm pid, unit)` per event (rate-limited to 50 lines/s with a "… N more" summary), and a top-5 summary every minute. Its exit code is 5 when it can't run (namespace or permission).

### Tests

- Unit: event-buffer parser on synthetic buffers (multiple events, truncated tail, overflow event, FID records with and without a name, bad lengths → error rather than panic); ring arithmetic (window sums, rollover, lossy propagation, key-cap eviction).
- Privileged (VM):
  - `fanotify_fd_mode_sees_close_write`: a child process writes 5 MiB and closes → the event arrives within 1 s with the correct path, PID, comm and +5 MiB delta.
  - `fanotify_probe_reports_capabilities`: record results for 8.8 and 8.10 in the handoff note; this answers whether FID modes were backported.
  - `watcher_namespace_guard` (`unshare -m` → refuses).
  - `watch_overflow_marks_lossy` (100k rapid close-writes → lossy minute flagged, thread survives).
  - `exclude_prefixes_respected`.
  - If FID modes are available: `fid_mode_create_delete_rename` (events with correct directory paths and names).
- Load: 2,000 close-writes/s for 60 s; CPU of the watcher thread under 2% of one core (record the measurement).

### Acceptance

- [ ] fd mode works on both VMs; FID mode is implemented and tested if either VM kernel supports it, otherwise left behind the probe with tests for the fallback path.
- [ ] No fd leaks: the fd count is stable over a 10-minute load test.

---

## W5-B — Process scanner: phantom files and open writers

**Read first:** `07-live-monitoring.md` §4, W1-D `procfs::scan_open_files` rustdoc.

### Interface

```rust
pub struct ProcScannerOptions { pub mounts: Vec<(RawPath, DevId)>, pub open_writers: bool, pub exclude_prefixes: Vec<RawPath> }
pub struct ProcScanResult { pub at: i64, pub fds_seen: u64, pub duration: Duration }
pub struct ProcScanner;
impl ProcScanner {
    pub fn new(cfg: ProcScannerOptions, attribution: Arc<AttributionCache>) -> Self;
    pub fn scan_once(&mut self, now: i64) -> io::Result<ProcScanResult>;
    pub fn phantom(&self, mount: Option<&RawPath>) -> Vec<PhantomSummary>;   // per mount: total + holders
    pub fn writers(&self, mount: Option<&RawPath>, window: Duration, limit: usize) -> Vec<OpenWriter>;
    pub fn take_samples(&mut self) -> (Vec<PhantomSample>, Vec<WriterSample>);
}
```

### Details

- One `scan_open_files(devs, want_flags = open_writers)` per pass. Phantom = `nlink == 0`. Deduplicate by `(dev, ino)`; holders are aggregated with each holder's attribution. The path is the readlink text with the trailing `" (deleted)"` stripped (the suffix is matched exactly at the end).
- Open writers: `flags & O_ACCMODE ∈ {O_WRONLY, O_RDWR}`, path not under `exclude_prefixes`. Keep history per `(dev, ino)` as a ring of the last 60 samples (size, disk, ts). `writers(window)` reports `disk(now) − disk(now − window)`, sorted descending. Entries not seen for 2 passes are dropped.
- Memory bound: at most 10,000 tracked writers with a 60-sample ring each (evict the smallest growth); about 15 MB worst case.
- **CLI:**
  - `phantom`: direct mode runs one pass (root for full coverage; non-root runs cover the readable subset) and prints total per mount plus holders (pid, comm, unit, path, size), with a hint to restart or HUP the holder. Daemon mode is wired in W6-A.
  - `writers --direct [--interval 10s]`: two passes separated by the interval, then shows growth.

### Tests

- Unit with a fake `scan_open_files` source (trait seam): dedup across processes, multiple holders, the exact `" (deleted)"` suffix (a path literally containing "(deleted)" in the middle is not stripped), window growth math, eviction.
- Privileged: `phantom_detects_deleted_open` (100 MiB, unlinked, held open by a child; attributed to the child's comm); `open_writer_growth` (appender detected; read-only holder ignored).

### Acceptance

- [ ] One pass over a VM with about 20k fds completes in < 500 ms (recorded).

---

## W5-C — *(Deferred.)*

---

## W5-D — Rates, alert engine, `now` model and renderer

**Read first:** `07-live-monitoring.md` §2, §6, §7, `08-interfaces.md` (`[alerts]`, metrics `tidemark_alert_firing`).

### Interface

```rust
// rates
pub fn slope_bytes_per_sec(samples: &[(i64, i64)]) -> Option<f64>;           // ≥ 3 samples, least squares
pub fn window_rate(samples: &[(i64, i64)], now: i64, window: Duration) -> Option<f64>;
pub fn time_to_full(avail: u64, rate: f64) -> Option<Duration>;
pub struct MountRates { pub mount: String, pub size: u64, pub used: u64, pub avail: u64,
                        pub rate_15m: Option<f64>, pub rate_1h: Option<f64>, pub ttf: Option<Duration> }
pub struct DirGrowth { pub root: String, pub path: RawPath, pub depth: u32, pub delta: i64, pub between: (String, String) }

// alerts
pub struct AlertInputs<'a> {
    pub mounts: &'a [MountRates], pub lvm: &'a [(String /*mount*/, LvmNow)], pub phantom: &'a [PhantomSummary],
    pub hot_dirs_window: &'a [(String /*mount*/, Vec<HotEntry>)],   // for hot_dir_growth (rule window)
    pub scans: &'a [ScanStatus], pub dir_growth: &'a [DirGrowth],
    pub watch_overflows_15m: &'a [(String, u64)], pub state_space_refused: bool,
}
pub struct AlertEngine;
impl AlertEngine {
    pub fn new(rules: &[AlertRule], renotify: Duration) -> Result<Self, AlertError>;   // validates rules
    pub fn evaluate(&mut self, now: i64, inputs: &AlertInputs) -> Vec<AlertEvent>;     // firing/resolved/renotify
    pub fn active(&self) -> Vec<AlertSummary>;
}
pub trait Deliver: Send { fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError>; }
pub struct JournalDeliver;                  // tracing-journald with structured fields
pub struct WebhookDeliver;                  // ureq; own thread; bounded queue (1,000); 5 s timeout; 3 retries

// now
pub struct NowInputs<'a> {
    pub host: &'a str, pub mounts: &'a [MountRates], pub lvm: &'a [(String /*mount*/, LvmNow)],
    pub hot_dirs_5m: &'a [(String /*mount*/, Vec<HotEntry>)], pub writers_5m: &'a [(String, Vec<HotEntry>)],
    pub phantom: &'a [PhantomSummary], pub open_writers: &'a [(String, Vec<OpenWriter>)],
    pub alerts: &'a [AlertSummary], pub scans: &'a [ScanStatus],
    pub watch_modes: &'a [(String, String, u32 /*lossy 15m*/)], pub degraded: &'a [String],
}
pub fn build_now(i: &NowInputs, now: i64) -> NowReport;
pub fn render_now_text(r: &NowReport, width: usize, color: bool) -> String;
```

### Details

- Rule kinds and semantics exactly as `07` §6.1–6.2: `for` / `resolve_for` hysteresis per `(rule, subject)`, and re-notify while firing. Path globs (`hot_dir_growth`) and VG/pool patterns (`thin_pool_pct`) use `globset`. Threshold strings are parsed with `tidemark_core::units` (`"50GiB/h"`).
- **Default rules** (`07` §6.4) come from pure `tidemark_core::config::default_rules(cfg)`, wrapped or re-exported by W5-D. The config layer merges them with user rules: a user rule with the same `name` overrides. Follow `07` §6.4 for stable names, scheduled-root stale defaults and collector availability, and `08` §3 for required fields and numeric bytes/s thresholds.
- `render_now_text` reproduces the layout in `07` §7, wrapping to `width`. With `color = false`, output is plain ASCII-safe (apart from the user's path bytes, which are rendered lossily).

### Tests

- `slope_bytes_per_sec` on linear, noisy, sawtooth and constant series; fewer than 3 samples gives `None`; `time_to_full` edge cases (zero, negative rate).
- Alert state machine: table-driven timelines (condition flaps shorter than `for` never fire; resolve after `resolve_for`; re-notify cadence; multiple subjects independent).
- Webhook: a local `tiny_http` test server receives the JSON body; timeouts and retries; queue overflow drops the oldest and counts it.
- `render_now_text` `insta` snapshots for: healthy, filling, degraded (watcher fd mode), and multi-mount.

### Acceptance

- [ ] 100% branch coverage of the alert state machine (`cargo llvm-cov` report attached, or an equivalent argument in the handoff note).

---

## Wave 5 gate checklist

- [ ] Privileged suite green on both VMs; fanotify capability findings for 8.8 and 8.10 recorded in `docs/handoff/W5-gate.md`.
- [ ] Manual check on a VM: run `tidemark watch` while a script writes files under two units (`systemd-run --unit=fake-report …`); attribution shows the unit names.
- [ ] Tag `wave-5-done`.
