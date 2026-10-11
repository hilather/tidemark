# 02 — Architecture

## 1. The idea in one paragraph

Fast Mac and Windows disk tools are fast because they **don't walk the tree file by file**. They read filesystem metadata in bulk, keep an index, and update it from a change feed. Tidemark does the same three things with what Linux and XFS offer:

| Mac/Windows trick | Tidemark equivalent on Rocky 8 + XFS |
|-------------------|--------------------------------------|
| Read the metadata table in bulk (NTFS MFT, APFS bulk attributes) | **XFS bulkstat**: inode attributes in large, on-disk-ordered batches, joined with a `getdents`-only namespace walk |
| Persistent index (Spotlight) | An **immutable SQLite snapshot per scan**: per-directory aggregates, notable files, hard-link sets |
| Change journal (FSEvents, USN journal) | **fanotify** (where writes land, and who writes them) plus 30-second free-space sampling |

On top of that sit things the Mac tools don't need: hard-link-correct accounting, LVM and Postgres awareness, and throttling that keeps a RAID 5 array usable while scanning.

**Tidemark is strictly read-only.** It changes no mounts, fstab entries, LVM, kernel parameters or file metadata. Installing it is just a package plus a service.

## 2. Layers

| Layer | Answers | Mechanism | Latency | Cost |
|-------|---------|-----------|---------|------|
| **L0 — Sampling** | "How full is each filesystem/volume, and how fast is it filling?" | `statvfs` per mount every 30 s; LVM (`lvs`/`vgs`) every 5 min | Seconds | Negligible |
| **L1 — Index** | "Where exactly is the space? What grew since yesterday? What would deleting X free?" | Scheduled scans (walk or bulk) → snapshot DB | Minutes per scan; queries instant | Throttled metadata reads, in quiet windows |
| **L2 — Live** | "Who is writing what, right now? What's hidden?" | fanotify watcher, `/proc` scanner, Postgres collector | ≤ 60 s | Low; bounded |

Each layer works without the others. Without the daemon, snapshot queries still work from the CLI.

**Tracked by default:** the two filesystems `/` and `/data`. Each is scanned, watched and sampled separately. Because scans never cross filesystems, scanning `/` never descends into `/data`, `/proc`, `/sys`, `/run` or tmpfs mounts.

## 3. Component diagram

```
                              ┌──────────────────────────── tidemarkd (root, systemd) ────────────────────────────┐
  Kernel / storage            │                                                                                    │
  ────────────────            │   L0  FsSampler (statvfs, 30s) ─────┐                                              │
  statvfs  / and /data ───────┼──▶                                  │                                              │
  LVM (lvs/vgs JSON, 5m) ─────┼──▶ L0  LvmSampler ──────────────────┤                                              │
                              │                                     ├──▶ LiveState (per-minute rings) ──▶ RateEngine ──▶ AlertEngine ──▶ journald / webhook
  fanotify (mount marks ×2) ──┼──▶ L2  Watcher (/ and /data) ───────┤             │                                │
  /proc/*/fd, fdinfo ─────────┼──▶ L2  ProcScanner (60s) ───────────┤             ├──▶ MetricsExporter ──▶ node_exporter textfile (+ optional HTTP)
  PostgreSQL (unix socket) ───┼──▶ L2  PgCollector (5m / 1h) ───────┘             └──▶ ApiServer (unix socket, JSON lines) ◀── tidemark now/hot/lvm/...
                              │                                                                                    │
  dirs + inodes ──────────────┼──▶ L1  Scheduler ─▶ Scanner (walk | bulk) ─▶ Aggregator ─▶ SnapshotWriter          │
  (getdents64/statx/bulkstat) │                       ▲                                       │                     │
                              │       Throttle gate ◀─┴── /sys/dev/block/*/stat (leaf devs    │                     │
                              │                            under LVM → md/disks)              ▼                     │
                              │                     timeseries.db ◀── samples, scan catalog, dir series             │
                              │   First-boot report (once per staged deploy) ──▶ /var/lib/tidemark/deploy/          │
                              └────────────────────────────────────────────────────────────────────────────────────┘
                                                                │ snapshots/<root>/<scan_id>.db (immutable, read-only)
                                                                ▼
                                     tidemark CLI: probe · scan · top · files · wide · diff · links · export · tui · lvm · deploy …
                                     (snapshot queries work without the daemon)
```

## 4. Repository and crate layout

```
tidemark/
├── Cargo.toml                 # workspace
├── rust-toolchain.toml        # pinned stable toolchain
├── AGENTS.md                  # conventions for coding agents (template in agents/AGENTS.md)
├── crates/
│   ├── tidemark-core/         # shared types, config schema, RawPath, units, errors. No I/O.
│   ├── tidemark-sys/          # ALL unsafe: XFS ioctls (geometry, bulkstat), fanotify, ioprio. Safe wrappers only.
│   ├── tidemark-fs/           # mountinfo, device + LVM resolution, probe, walker, throttle, procfs parsers
│   ├── tidemark-scan/         # orchestrator, bulk pipeline, aggregator, link resolution
│   ├── tidemark-store/        # SQLite snapshot writer/reader, catalog, diff, time series, retention
│   ├── tidemark-live/         # fanotify watcher, proc scanner, rates, alerts, `now` model
│   ├── tidemark-pg/           # Postgres collector
│   ├── tidemark-cli/          # binary `tidemark` (commands, rendering, TUI, deploy)
│   ├── tidemarkd/             # binary `tidemarkd` (services, scheduler, API, metrics, first-boot report)
│   └── test-support/          # fixture DSL, oracles (du/find), loop-XFS and loop-LVM helpers
├── xtask/                     # ci, vm-test, gen-fixture, bench, rpm
├── packaging/                 # systemd, tmpfiles.d, sysusers.d, default config, RPM metadata, Grafana/Prometheus
└── docs/
    ├── plan/                  # this plan
    └── handoff/               # one note per completed task
```

**Rule:** every crate except `tidemark-sys` declares `#![forbid(unsafe_code)]`. Each `unsafe` block in `tidemark-sys` carries a `// SAFETY:` comment, and every FFI struct has a layout test.

### Dependency graph (crate level)

```
tidemark-core  ◀── everything
tidemark-sys   ◀── tidemark-fs, tidemark-scan (bulkstat), tidemark-live
tidemark-fs    ◀── tidemark-scan, tidemark-live, tidemark-cli, tidemarkd
tidemark-store ◀── tidemark-scan, tidemark-live, tidemark-cli, tidemarkd
tidemark-scan  ◀── tidemark-cli, tidemarkd
tidemark-live, tidemark-pg ◀── tidemark-cli, tidemarkd
(tidemark-fs never depends on tidemark-pg; the CLI composes the probe's Postgres section.)
test-support   ◀── dev-dependencies only
```

## 5. Process model

- **`tidemarkd`** runs as root under systemd. It owns all samplers, the watcher, the scan scheduler, the time-series DB writer, metrics output, the API socket and the first-boot report. It is **read-only** with respect to everything outside its own state directory.
- **`tidemark`** (CLI) runs as root or as a member of group `tidemark`.
  - Snapshot queries open snapshot files directly, read-only and immutable.
  - Live queries (`now`, `hot`, `lvm`, …) go to the daemon socket. Where possible they fall back to doing the work directly (for example `phantom` scans `/proc` itself, `lvm` runs `lvs` itself).
  - `deploy stage|verify|unstage` manage the service for the hotfix pipeline (`§10`).

### Threading inside the daemon (no async runtime)

| Thread | Work |
|--------|------|
| `main` | Config, signals, sd_notify, supervision, first-boot report |
| `sampler` | Every 30 s: `statvfs` for tracked mounts → LiveState + timeseries |
| `lvm` | Every 5 min: `lvs`/`vgs` JSON report → LiveState + timeseries |
| `watcher-<mount>` | Blocking `read()` on the fanotify fd → resolve → aggregate into per-minute rings (one per tracked mount) |
| `procscan` | Every 60 s: `/proc` walk → phantom and open-writer state |
| `pg` | Every 5 min (relations hourly): Postgres queries |
| `scheduler` | Starts scans in allowed windows; at most one scan at a time on the host |
| `scan-*` | Walker/bulkstat workers + aggregator (spawned per scan) |
| `ts-writer` | Single writer for timeseries.db; receives batched rows over a channel |
| `api` | Accepts Unix-socket connections; short-lived handler threads |
| `metrics` | Every 60 s: render textfile atomically; optional tiny HTTP server |
| `alerts` | Every 30 s: evaluate rules; deliver notifications |

Shared state is `Arc<RwLock<LiveState>>` with small critical sections. Cross-thread data goes through `crossbeam-channel`. See ADR-005.

## 6. Data flows

- **Scan (L1):**
  1. The scheduler or CLI requests a scan of a root (`/` or `/data`).
  2. The orchestrator picks walk or bulk mode (`04-scanner.md`).
  3. Workers produce per-directory records.
  4. The aggregator resolves hard-link sets and rolls up totals.
  5. The writer streams rows into `<scan_id>.db.tmp`, builds indexes, fsyncs and renames.
  6. The catalog row and `dir_series` rows (dirs down to `series_depth`) go into `timeseries.db`.
- **Live (L0/L2):** samplers and watchers feed `LiveState`, which holds in-memory rings: 60 one-minute buckets plus recent raw samples. The rate engine computes slopes, the alert engine evaluates rules, the ts-writer persists, and the API and metrics read.
- **Query:** the CLI opens snapshots read-only (SQLite `immutable=1`), with no daemon needed. Live views call the API.

## 7. On-disk layout

```
/etc/tidemark/tidemark.toml          config (0640 root:tidemark)
/etc/tidemark/pg.pass                optional Postgres password file (0600 root)
/var/lib/tidemark/                   state_dir (0750 root:tidemark). On the root filesystem by default,
│                                    which keeps Tidemark's own writes off /data. Excluded from scans and watches.
├── timeseries.db (+ -wal, -shm)     samples, scan catalog, dir series, alerts
├── snapshots/<root-slug>/<scan_id>.db        immutable snapshots (0640)
├── snapshots/<root-slug>/<scan_id>.db.tmp    in-progress scan (deleted on failure)
├── deploy/staged.json, deploy/firstboot.json deployment state (FR-25)
└── tmp/                             scratch
/run/tidemark/api.sock               API socket (0660 root:tidemark)
/var/lib/node_exporter/textfile_collector/tidemark.prom   metrics (configurable)
```

`root-slug` is the root path with `/` replaced by `_`, so `/data` becomes `_data` and `/` becomes `_` (`_` inside names is escaped as `%5F`, so the mapping is injective; W1-E `root_slug` is normative). `scan_id` is `YYYYMMDDTHHMMSSZ-<8 hex>`.

**Space note:** snapshots of `/data` (millions of directories) can reach a few GB. The space guard (`05` §8) protects the root filesystem, and `state_dir` can be pointed elsewhere if `/` is small.

## 8. Security model

- **Capabilities.** The daemon needs root-equivalent capabilities, and the systemd unit bounds the set to exactly these:
  - `CAP_SYS_ADMIN` for bulkstat and fanotify.
  - `CAP_DAC_READ_SEARCH` to traverse every directory.
  - `CAP_FOWNER` for `O_NOATIME` on directories it doesn't own.
  - `CAP_SYS_PTRACE` to read other processes' `/proc/<pid>/fd`.
- **The daemon must run in the host mount namespace.** fanotify mount marks attach to a specific mount object. systemd options that create a private mount namespace (`ProtectSystem=`, `ProtectHome=`, `PrivateTmp=`, `ReadWritePaths=`, …) give the service *copies* of the mounts, and the watcher would silently see nothing.
  - Hardening instead uses capability bounding, `NoNewPrivileges`, `RestrictNamespaces`, `SystemCallArchitectures`, `LockPersonality` and `RestrictAddressFamilies`.
  - `tidemarkd` checks that `/proc/self/ns/mnt` equals `/proc/1/ns/mnt` and refuses to start the watcher otherwise. (ADR-013.)
- **LVM access.** It is read-only: the `lvs`/`vgs` reporting commands, which take a brief shared lock and never write metadata.
- **Network.** No TCP listener by default. The optional metrics listener binds to `127.0.0.1` only.
- **Path names are sensitive.** Snapshot files and the API expose them (customer names in report paths), so access is limited to root and group `tidemark`.
- **No content reads.** Tidemark never opens regular files to read data. fanotify hands the daemon an fd per event, and that fd is only `fstat`-ed and closed.

## 9. Degradation matrix

| Condition | Behaviour |
|-----------|-----------|
| fanotify FID/name reporting unavailable (likely on 4.18) | Watcher runs in fd mode: close-write events with path and PID. Creates/deletes/renames are not seen; net change comes from L0 and scans. |
| Not root / missing capability | Walk mode only over the readable subset; snapshot marked `degraded`. No watcher, no procscan of other users. |
| Not XFS, or bulkstat unavailable | Walk mode. |
| Scan memory estimate exceeds budget | Walk mode. |
| Not on LVM, or `lvs` missing | LVM section shows "n/a"; everything else unaffected. |
| Postgres unreachable | Collector backs off; `now` shows "pg: unreachable since …". |
| Daemon down | CLI snapshot queries still work; live commands report "daemon unavailable" (exit 4); `phantom`/`lvm` fall back to direct mode. |
| State filesystem low on space | Snapshots skipped (alert raised); time-series compaction; samples continue. |
| fanotify queue overflow | Window marked `lossy`; counter metric; nothing else breaks. |

## 10. Deployment model (FR-25)

Tidemark ships as an RPM through the existing hotfix pipeline:

1. **Deploy time** (no reboot needed yet):
   - `dnf install tidemark-<ver>.rpm`.
   - `tidemark deploy stage --from <file>` runs pre-flight checks. On any blocking failure it changes nothing and exits non-zero.
   - On success, it installs the config (backing up any previous one) and runs `systemctl enable tidemarkd` **without starting it**.
   - It records `deploy/staged.json`.
2. **Next reboot:** the service starts, samples immediately, starts the watchers, writes `deploy/firstboot.json` (component health), and schedules the first scans in their windows.
3. **After the reboot:** `tidemark deploy verify [--wait 5m]` returns 0 when the first-boot report is healthy, for the pipeline's post-reboot check.
4. **Back-out:** `tidemark deploy unstage` (stop and disable, keep data) or `dnf remove tidemark`.

There are no fstab, LVM or kernel changes, so a failed deploy can't affect boot. `deploy stage --start-now` starts immediately for hosts that won't reboot soon.

## 11. Technology choices (details in ADRs)

- **Language and build:** Rust, edition 2021, pinned stable toolchain; built on a `rockylinux:8` container against glibc 2.28.
- **Syscalls:** `rustix` (safe `getdents64`/`statx`/`openat`), `libc` for the rest. All FFI lives in `tidemark-sys`.
- **Storage:** `rusqlite` with `bundled` SQLite, so there is no system SQLite dependency.
- **Concurrency:** `std::thread`, `crossbeam-deque` (work stealing), `crossbeam-channel`, `parking_lot`, and `rayon` for sorts only.
- **CLI/TUI:** `clap` (derive), `ratatui` + `crossterm`.
- **Observability:** `tracing`, `tracing-subscriber`, `tracing-journald`.
- **Config and serialisation:** `serde`, `toml`, `humantime-serde`, `bytesize`, `serde_json`. JSON is also used to parse LVM's `--reportformat json` output.
- **Other crates:**
  - `postgres` (sync client)
  - `ureq` for webhooks
  - `tiny_http` for the optional metrics listener
  - `sd-notify`
  - `xxhash-rust` (xxh3) for path hashes
  - `rustc-hash` for hot maps
- **Tests:** `tempfile`, `assert_cmd`, `insta`, `proptest`, `cargo-fuzz`.
- **Packaging:** `cargo-generate-rpm`.
