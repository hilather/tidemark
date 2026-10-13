# Staging performance tools

Run these tools only on a disposable staging copy of the Rocky Linux 8/XFS workload. Administrators use sudo; there is no access-granting tidemark group. Never drop caches on a developer workstation or production server. No PostgreSQL-specific workload or gate is required.

Use the pinned toolchain and cached dependencies. The harness measures `release/tidemark` in the cargo target directory, or the CLI given with `--tidemark PATH`. It builds the default CLI with `cargo build --offline --locked --release -p tidemark-cli` when missing. Rebuild it explicitly after source changes. GNU `du`, `date`, `getconf`, procfs and sysfs are required; install GNU `/usr/bin/time` on staging for rusage evidence. Keep results/state on a different filesystem from the fixture if possible; `--out` must be outside the fixture. The harness creates a unique private config, socket and state directories, avoiding a system daemon.

Create existing empty roots on the staging XFS filesystem, then run from the repository root:

```sh
cargo build --offline --locked --release -p xtask -p tidemark-cli
mkdir /data/tidemark-5m /data/tidemark-20m
sudo target/release/xtask gen-fixture --profile reports --inodes 5000000 --root /data/tidemark-5m --seed 714
sudo target/release/xtask gen-fixture --profile reports --inodes 20000000 --root /data/tidemark-20m --seed 714
sudo target/release/xtask bench --root /data/tidemark-5m --modes du,walk,bulk --cache warm,cold --drop-caches-on-this-host --out docs/perf/5m
sudo target/release/xtask bench --root /data/tidemark-20m --modes du,walk,bulk --cache warm,cold --drop-caches-on-this-host --out docs/perf/20m
sudo target/release/xtask bench --root /data/tidemark-20m --modes walk,bulk --cache warm --throttle normal --out docs/perf/normal
sudo target/release/xtask bench --root /data/tidemark-5m --watcher-overhead --duration 60 --mutate-rate 1000000 --out docs/perf/watcher
```

Use `--tidemark PATH` to measure an existing CLI without building the release CLI.

Cold mode flushes writes and writes `3` to `/proc/sys/vm/drop_caches` before **each** measured command, including each du variant. Explicit `--drop-caches-on-this-host` is mandatory. It requires effective uid 0, refuses a running tidemarkd or one-minute load exceeding available CPUs, and prints the host-wide action. `--force` overrides daemon/load guards only; use it only after recording why on staging. Warm runs execute an unmeasured warm-up for each invocation. The cold performance gate uses `--throttle max` (the default).

The inode target counts unique regular files, symlinks, directories (including root), marker and summary files. Hard-link aliases add directory entries, not inodes. The marker records profile, seed, count and dense policy. Re-running in a marked root replaces its contents; keep no unrelated data there. Empty existing roots and valid marked roots are accepted; `/`, symlink components, parent components, unresolved mount topology and nested mounts are refused. Directory operations use no-follow fd capabilities; procfs must be mounted. Do not concurrently remount or rename the root while generating. No filesystem/LVM/service configuration is changed.

Generation uses SplitMix64 and Box–Muller with log-normal sigma 2, median 32 KiB and cap 2 GiB. Files above 1 MiB are sparse unless `--dense`; smaller files contain zero-filled data. Three directories scale to 100,000 original-file entries at 5M+, with at least five other report directories averaging about 35,000 originals at scale; hard-link aliases increase their widths. Small targets scale these widths proportionally. Every fifth regular file receives two to four aliases in distinct other report directories; one tenth of non-directory unique data inodes are symlinks, alternating directory/file targets. All paths, sizes and aliases are deterministic for seed/count/dense policy. Directory allocation and timestamps remain filesystem-dependent. Creation retains no all-paths list.

The stdout summary and `.tidemark-fixture.json` contain the same unique-inode counts, link sets/aliases and data/directory apparent and allocated bytes. Control-file bytes are included; the summary file is padded to 4096 bytes to make its own size non-circular. The summary is the initial fixture; mutation is reported separately. Mutation uses a bounded 1024-slot ring with 25% create, append, hard-link and delete operations (delete removes both aliases). Each create/append closes a 4 KiB write. `--duration` is seconds; without it the generator continues until interrupted. The rate is logical operations/second, not close-write events/second. There are two close-write events per four logical operations. SIGINT/SIGTERM terminate the generator using the normal process action; a later marked-root run cleans up interrupted work.

Watcher overhead compares the mutation throughput without and with a private foreground `tidemark watch --events close_write` child, checking child liveness after one second. The command rebuilds the same fixture for each run; CPU/wall results include that preparation, while the separately reported mutation throughput excludes it. A filesystem fanotify mark may cover the entire root mount: dedicate the staging filesystem. Verify actual event delivery in watcher.stdout; liveness alone is not watcher readiness. Use a rate high enough that the requested cap does not hide throughput loss, and repeat trials in alternate order. Foreground event rendering is included in this experiment; retain separate daemon watcher CPU evidence for the R4-8 budget. This experiment was not run on the developer host.

Results are `results-<UTC-date>-<host>.md` and `.json`; unique per-run directories retain commands' stdout/stderr/time files and `block-stat.jsonl`. A second run on the same date overwrites the top-level report but retains the previous run's raw directory: use separate `--out` directories to preserve each report. GNU time records wall/user/system CPU and rusage maximum RSS. When it is absent, a labelled procfs fallback uses reaped child CPU ticks and a sampled RSS lower bound; that fallback cannot establish a peak-memory gate. The harness also samples aggregate process-tree RSS every 20 ms; short-lived peaks can be missed, so use cgroup memory.peak or external sampling for authoritative aggregate memory gates. Block series are streamed at one-second intervals, plus boundary samples, on sysfs leaf devices resolved through partition parents and slaves. Baseline is two seconds without a scan. Raw counters and elapsed intervals accompany utilisation (`io_ticks/elapsed`) and await (`read/write ms / completed read/write I/Os`). Non-block-backed developer filesystems have an empty leaf list, explicitly visible in JSON. Scanner process samples need not equal the adaptive controller's own samples.

Current `scan --json` publishes schema `tidemark.scan.v1`, counts and byte totals, errors/races/skips and mode/degraded status. It does **not** publish gate mean/minimum rate, throttled time or decreases. The harness preserves all CLI JSON and labels gate statistics unavailable; obtain those separately from instrumentation on staging without changing the CLI contract. GNU du prints only a byte total, so its raw result has no directory/file count. Record directory/file counts from the scanner summaries and reject unexpected differences/errors/degraded runs. The apparent-size du command is measured separately; use quiescent, flushed XFS fixtures to compare bulk apparent sizes (R3-16). No syscall count is claimed; collect it separately with `strace -c` if required by testing §7.

Record CPU model/count, RAM, kernel, OS, host load, XFS geometry/ftype/reflink, RAID/controller, leaf devices and schedulers, fixture seed/count/dense policy, git commit, toolchain, memory budget, configured utilisation/await targets, throttle settings, errors, mode selection, every raw result and profiling artifacts. Run `perf record` and retain walk/bulk/aggregation flamegraphs at both scales; write the top three hotspots and any tuning proposal as separately reviewed follow-up work.

Wave 7 gates (no automatic acceptance from these tools):

- Cold walk or bulk with throttle off at least 3× faster than allocated-byte `du -sx --block-size=1`, or an agreed revised target.
- Normal throttle leaf utilisation and average wait within configured targets except short excursions corrected by the controller.
- Scan peak RSS within configured `memory_budget` (default 1.25 GiB); verify both modes at 20M. A bulk memory failure needs W7-FIX-spill review.
- Watcher writer-throughput impact measured; R4-8 active watcher CPU budget is **5% of one core per 1,000 close-write events/second**, equivalent to 50 µs/event (10% at 2,000/s). This replaces the old 1%/2% thresholds. Measure actual events and CPU independently of writer throughput.
- Daemon steady-state RSS ≤300 MiB and idle CPU ≤2% of one core outside scans.
- 72-hour soak with default daemon, continuous generator mutation and hourly scans: RSS growth trend ≤5%, flat fd count, missed samples ≤1%, no unexplained journal errors and continuous metrics. Keep CPU/RSS/fd/event/sample/journal series. Watcher-backed alert-rule availability has known producer limitations and is not inferred from this soak.

For the soak on staging, run the generator with `--mutate-rate 2000` without duration; record actual close-write rates and stop it with SIGINT. That command regenerates the marked fixture before entering mutation. Use a separate hourly scan schedule managed by the operator.

| Date / host / commit | Inodes | Mode / cache / throttle | Wall / CPU | Peak RSS | Disk / apparent bytes | Errors / degraded | Leaf util / await | Watcher events / CPU / throughput | Gate evidence |
|---|---:|---|---|---|---|---|---|---|---|
