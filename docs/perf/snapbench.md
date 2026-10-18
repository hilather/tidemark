# Synthetic snapshot benchmark: v1 baseline

## Method

`snapbench` generates metadata only, using SplitMix64 seed 42 and a fixed scan
clock of 1,780,000,000 Unix seconds. The default fixture has 2,000,000 directories,
200,000 notable single-link files, and 500,000 hard-link instances (250,000 complete
same-directory pairs). Two directories have 100,001 direct children each; most
remaining branches have six-directory stems and leaves at depth seven. An exact
1,000-directory subtree and a depth-12 path are included. Names are sorted raw
bytes with lengths 4, 8, 17 and 41, repeating source/archive/data suffixes.
Allocated sizes follow a bounded, 512-byte-rounded log-normal distribution;
mtimes span two years, with charged age buckets and subtree totals rolled up.
The writer is the public scan-path `SnapshotWriter`, including its indexes and
publication work. Counts are capped at five million per CLI count argument.

The comparison snapshot changes directory inode bytes in every hundredth row
(1% at full size), then recomputes ancestor totals. Two renamed leaf subtrees
produce two additions and two removals in path-based diff results.

Each query has five timed fresh-handle runs and five warm runs. Fresh-handle
latency includes `Snapshot::open`, query, and handle close. **The OS page cache is
not dropped**; this is not physical cold-disk latency. Warm runs share one
handle after an untimed query. Median is the third sorted sample, p95 is the
fifth (nearest-rank p95 for five samples). The public diff API opens the old
snapshot internally even during warm runs. Query results are consumed without
CLI rendering. Ncdu exports the entire snapshot to an I/O sink, including JSON
serialization but excluding output-device latency.

| Query | Public operation |
|---|---|
| children_root | `children_page(0, Disk, descending, first page of 50)` |
| children_wide | Same page under a 100,001-child directory |
| top2 / top4 | `subtree_dirs(0, depth, Disk, 1_000_001)`, as CLI top fetches candidates |
| files_root / files_subtree | `subtree_files(id, default filter, 50)` |
| path12 | `dir_by_path` for an existing depth-12 path |
| diff | Public `diff(new, old, root, Disk, unlimited depth, 50)` |
| ncdu | `export_ncdu` under root, no depth limit |

The human table goes to stderr; `--json` emits JSON on stdout. `OUT/report.json`
is always written with metadata and all sorted samples. The snapshots remain in
`OUT` for inspection; remove that benchmark output directory after retaining the
report. Use a fresh output directory per run; existing snapshot-name collisions
are refused by the writer.

## Machine and command

Measured 2026-10-17 on `brewtop`: Intel Core i7-8750H at 2.20 GHz (6 cores,
12 logical CPUs), 62.3 GiB RAM, Linux 7.2.5-3-omarchy x86_64. The output resides
on the host's `/dev/mapper/root` filesystem. Rust 1.98.0; workspace release
profile uses thin LTO and one codegen unit. This shared developer machine is not
an isolated benchmark host; the numbers are a local comparison baseline.

```sh
CARGO_NET_OFFLINE=true timeout 1500 cargo run --offline --release \
  -p tidemark-store --example snapbench -- \
  --layout v1 --dirs 2000000 --files 200000 --links 500000 \
  --seed 42 --out /tmp/tidemark-snapbench-full --query all --json
```

The actual sandbox invocation used the existing retained-directory entry point,
because release ancestry checks reject the sandbox's unmapped owner (uid 65534)
on `/`. The ordinary command above failed before writing a snapshot. No library
policy was changed. The successful invocation is:

```sh
mkdir -m 700 /tmp/tidemark-snapbench-full
exec 3</tmp/tidemark-snapbench-full
CARGO_NET_OFFLINE=true timeout 1500 cargo run --offline --release \
  -p tidemark-store --example snapbench -- \
  --layout v1 --dirs 2000000 --files 200000 --links 500000 \
  --seed 42 --out /proc/self/fd/3/. --query all --json
```

## Results

The full-size run completed successfully under `timeout 1500` (approximately
15.5 minutes including generation and all repetitions). Reported shape: median
depth **7**, maximum depth **12**, wide-directory children **100,001**,
and notable-subtree directories **1,000**.

| Query | Fresh median ms | Fresh p95 ms | Warm median ms | Warm p95 ms |
|---|---:|---:|---:|---:|
| children_root | 24.637 | 27.551 | 24.654 | 25.447 |
| children_wide | 35.494 | 42.761 | 40.757 | 76.427 |
| top2 | 1465.106 | 2266.071 | 967.294 | 1066.926 |
| top4 | 1107.239 | 1120.611 | 1283.269 | 1347.978 |
| files_root | 2740.861 | 3183.925 | 2909.809 | 3456.611 |
| files_subtree | 2.604 | 2.761 | 1.420 | 1.560 |
| path12 | 1.092 | 1.167 | 0.052 | 0.063 |
| diff | 11211.209 | 13006.853 | 9967.726 | 12768.459 |
| ncdu | 69513.649 | 102254.268 | 36602.273 | 51366.590 |

Original snapshot: **600,735,744 bytes** (572.906 MiB).
Comparison snapshot: **600,735,744 bytes** (572.906 MiB).
Process peak RSS: **1,916,076 KiB** (1871.168 MiB), including generation/writes.
Both generated snapshots were deleted after retaining the JSON report and logs.
Measurements overlapped workspace validation on this shared machine; the p95
spread should not be interpreted as isolated-host variance.


## Assumptions and limits

Hard links are local pairs, so this fixture exercises the link tables and their
space cost without modeling cross-directory sharing. Notable rows are the
single-link subset; other inodes are represented by link-set/instance rows.
Small `--dirs` smoke runs shrink the two wide groups; if no 1,000-directory
subtree exists, the subtree-file target falls back to root. Full-size runs meet
the specified shape. Peak RSS is process `VmHWM` from `/proc/self/status`, so it
includes generation and both writes as well as queries, not just reader memory.
The harness retains at most one generated tree at a time.

Adding a future format requires extending the layout validation, writer dispatch,
and `open` dispatch; query bodies and timing/reporting stay shared. No v2 reader
or actual cold-file path is available in this baseline checkout. The future v2
acceptance threshold is `max(1.25 × v1, 100 ms)` per corresponding query; no v2
comparison is claimed here.

## v2

The method is unchanged: same generator, seed, fixture shape, queries, repetition
counts and statistics. The layout is selected with `--layout v2`
(the default remains `v1`); the original and comparison snapshots are both written in
the selected layout. The acceptance rule from the design (`docs/design/15-snapshot-v2.md`
§8) is v2 ≤ max(1.25 × v1, 100 ms) per query, compared on the same host in the same
session.

```sh
mkdir -m 700 /tmp/tidemark-snapbench-v2
exec 3</tmp/tidemark-snapbench-v2
CARGO_NET_OFFLINE=true timeout 1500 cargo run --offline --release \
  -p tidemark-store --example snapbench -- \
  --layout v2 --dirs 2000000 --files 200000 --links 500000 \
  --seed 42 --out /proc/self/fd/3/. --query all --json
```

Measured on integration `61abc4c` (the v2 reader now on main), host release build, OS page cache not dropped.

| Query | Mode | v1 median ms | v1 p95 ms | v2 median ms | v2 p95 ms | Limit ms | Result |
|---|---|---|---|---|---|---|---|
| children_root | fresh_open | 9.2 | 424.2 | 3.1 | 3.4 | 100.0 | pass |
| children_root | warm | 8.5 | 8.7 | 1.6 | 1.6 | 100.0 | pass |
| children_wide | fresh_open | 15.6 | 25.4 | 44.5 | 45.2 | 100.0 | pass |
| children_wide | warm | 14.6 | 14.7 | 41.1 | 44.6 | 100.0 | pass |
| top2 | fresh_open | 773.3 | 895.5 | 473.9 | 509.2 | 966.7 | pass |
| top2 | warm | 760.6 | 791.8 | 476.9 | 482.3 | 950.7 | pass |
| top4 | fresh_open | 767.3 | 778.5 | 501.6 | 509.6 | 959.1 | pass |
| top4 | warm | 803.5 | 826.8 | 513.3 | 521.3 | 1004.3 | pass |
| files_root | fresh_open | 879.9 | 902.6 | 204.3 | 212.4 | 1099.9 | pass |
| files_root | warm | 934.2 | 942.2 | 25.3 | 25.7 | 1167.8 | pass |
| files_subtree | fresh_open | 1.1 | 1.3 | 4.8 | 4.9 | 100.0 | pass |
| files_subtree | warm | 0.4 | 0.5 | 0.0 | 0.0 | 100.0 | pass |
| path12 | fresh_open | 0.5 | 0.5 | 2.4 | 2.6 | 100.0 | pass |
| path12 | warm | 0.0 | 0.0 | 0.0 | 0.0 | 100.0 | pass |
| diff | fresh_open | 4453.2 | 4492.9 | 2660.1 | 2837.3 | 5566.5 | pass |
| diff | warm | 4319.5 | 4491.6 | 2590.8 | 3020.2 | 5399.4 | pass |
| ncdu | fresh_open | 72584.6 | 73225.9 | 1296.4 | 1362.5 | 90730.8 | pass |
| ncdu | warm | 72094.1 | 73986.6 | 1365.4 | 1415.9 | 90117.6 | pass |

Snapshot bytes (old + changed copy): v1 600,735,744 + 600,735,744; v2 93,171,712 + 93,184,000. Process peak RSS (includes generating the fixture): v1 1,916,436 KiB; v2 1,012,076 KiB.
