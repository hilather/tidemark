# timeseries.db size: v1 baseline

PR A records the current `TimeSeries` write path. It does not change the schema, the insert statements, compaction, or procscan. Later PRs rerun the same command and compare the fields below.

## Host observation

These figures are from a production `timeseries.db` on Matt's host. This harness did not open that file.

| Item | Value |
|---|---:|
| Page size | 4 KiB |
| `page_count` | 57,073 |
| Main file | 233,771,008 bytes (57,073 × 4096, about 223 MiB) |
| Freelist | 0 |
| `writer_samples` | 166 MB |
| `writer_ts` | 20 MB |
| `phantom_samples` | 26 MB |
| `phantom_ts` | 2 MB |

The four named objects were reported as about 214 MB. The rest of the file is other dbstat names and rounding. `writer_ts / writer_samples` is 20/166 = 12.0%. `phantom_ts / phantom_samples` is 2/26 = 7.7%. `phantom_samples / writer_samples` is 26/166 = 15.7%.

## Calibration

The shipped defaults and the host ratios are why the v1 profile is 16 processes, 8 files, a 60 s interval, a 7-day append, a 6-day hot window in the retention phase, 64-byte paths, and 150‰ churn.

| Knob | Shipped | Harness v1 |
|---|---|---|
| Procscan interval | 60 s (`ProcscanConfig` default, `docs/plan/08-interfaces.md` `[procscan]`, `docs/plan/07-live-monitoring.md` §4) | `--interval-secs 60` |
| Writer and phantom retention | 7 days, delete only, no downsample (`SampleRetention::default` `hot_minutes` = 604800 s, `docs/plan/05-storage.md` §6.1) | Append duration 604800 s, one raw window |
| Open files per pass | Not a config key. Procscan admits at most `MAX_WRITERS` (10,000) inodes. The host did not publish a histogram | 16 × 8 = 128 every pass |
| Path length | Not stored. Values are `/proc` fd targets | 64 bytes on every row |

The retention phase does not use the shipped 7-day window. Append-only data already covers exactly 7 days: every row has `ts >= start_ts`, and `compact` deletes `ts < now - hot_minutes`. With `now = start_ts + 604800` and `hot_minutes = 604800`, the cutoff is `start_ts` and the delete matches nothing. The phase therefore passes `hot_minutes` of 518400 s (6 days), so the cutoff is `start_ts + 86400` and the first day is removed. That is only so freelist reuse is visible. A daemon on the shipped default would keep the whole append-only week.

The indexes are only `writer_ts(ts)` and `phantom_ts(ts)`. Index bytes per row stay near 15 no matter how long the path is, so the index-to-table ratio is a row-width measurement. The host's 12.0% implies a writer row near 15 / 0.12 ≈ 125 bytes. The first harness profile used length classes 96, 128, 160, 192, and 256 (mean 166.4). It measured `writer_samples` 190,365,696 bytes / 806,400 rows (236 bytes/row) and `writer_ts` 11,993,088 (6.3% of the table). Those rows were wider and fewer than the host.

The path template is `{mount}/srv/{comm}.service/pid-{pid}/slot-{slot}/gen-{generation}/app.log`. For `redis-server`, pid 1005, and generation 11520 (the reuse-day pass count the profile rejects shorter classes against), that core is 64 bytes. A shorter class cannot be used. v1 therefore sets `--path-bytes 64` and every emitted path is exactly 64 bytes (1,696,274 rows across append and the reuse day). There is no remaining spread under this template. The mean is 64.

128 open files × 10,080 passes = 1,290,240 writer rows. The measured index is 19,206,144 / 1,290,240 = 14.89 bytes/entry, which is 19.2 MB against the host's 20 MB. The table is 165,539,840 bytes against 166 MB. The ratio is 19,206,144 / 165,539,840 = 11.6%. Churn is 150‰ so phantom rows are 193,929 / 1,290,240 = 15.0% of writer rows, next to the host's 15.7% table ratio. The phantom table is 24,068,096 bytes against 26 MB.

What is still different:

- Writer index/table is 11.6% versus 12.0%. The harness inode is always an 8-byte integer (`pid << 40`), comm and unit come from a fixed 15-name list, and every path is padded out to the 64-byte core. The host figures are rounded, not dbstat bytes.
- `phantom_ts` is 2,859,008 bytes versus the host's 2 MB, while the phantom table is close to 26 MB. The host's phantom index ratio is 7.7%, so those rows are wider than its writer rows (or the smaller index is packed less tightly). This generator uses one path length for both tables, so the phantom index ratio matches the writer ratio. No host path histogram is available to split them.
- The append-only file is 211,718,144 bytes versus 233,771,008. The difference is the other host objects plus that phantom-index gap.
- One mount, `dev` 1, and `(pid, slot)` order. Procscan orders by `(dev, ino)` and can use several mounts. The ts indexes do not store the path, so order moves page splits more than the index ratio.
- The open-file count is flat. 20 MB / 14.89 bytes / 10,080 passes is about 133 files if the host index were exactly 20,000,000 bytes. 128 is that neighbourhood, inside the 10,000-writer cap.

PR B–D wins still carry. Dropping `writer_ts` removes 19.2 MB here and about 20 MB on the host; the row counts are the same order (1.29 million here, about 1.34 million if the host index is 20 MB at 14.89 bytes/entry). Dropping `phantom_ts` removes 2.86 MB here and about 2 MB on the host. Interning paths collapses a blob that is written once per pass per open file; these 64-byte paths are repeated 10,080 times, and the host's 12% ratio says its writer rows are only slightly narrower, so that saving is not an artifact of the old 166-byte padding. A `WITHOUT ROWID` primary key on `(ts, pid, dev, ino)` replaces the ts index. That does not depend on path length. A percentage measured on this profile can be read against the host. The old 6.3% figure could not.

## What the harness measures

`cargo xtask ts-size` builds synthetic writer and phantom rows and writes them with `TimeSeries::insert_phantom` and `TimeSeries::insert_writers`. Each pass is one phantom transaction, then one writer transaction, chunked at `tidemarkd::services::tswriter::MAX_BATCH_ROWS` (20,000). Empty passes skip the empty call. v1 has 128 writer rows per pass, so a chunk boundary is never reached.

Rows are ordered by `(pid, slot)`. That is not procscan's growth order or `(dev, ino)` order. One mount is used. Indexes are only `phantom_ts(ts)` and `writer_ts(ts)`, so path length changes the table and barely changes the index.

After the writer connection is dropped, a second connection runs `PRAGMA wal_checkpoint(TRUNCATE)` and refuses the report unless the result is not busy, every WAL frame was copied, the `-wal` file is missing or empty, `main_file_len == page_count * page_size`, and `SUM(dbstat.pgsize) + freelist_count * page_size == page_count * page_size` (one reserved lock page is allowed only past 1 GiB). dbstat bytes are allocated pages, not payload. `shm` is not part of the comparison.

The command writes `OUT/timeseries.db` and `OUT/report.json`. It refuses an existing database. `--out` must not contain `..`; the directory is created mode `0700` and must pass the state-directory ancestry checks.

`report.json` keys `objects` and `rows` by name. Serde writes those keys in sorted order, the same order as `dbstat GROUP BY name ORDER BY name`. Diff the name, not an array index.

### Generator

| Parameter | v1 |
|---|---|
| `--profile` | `v1` |
| `--processes` | 16 |
| `--files-per-process` | 8 (128 open files per pass) |
| `--interval-secs` | 60 |
| `--duration-secs` | 604800 (10,080 passes, seven days) |
| `--seed` | 42 |
| `--churn-permille` | 150 |
| `--path-bytes` | 64 |
| `--mount` | `/data` |
| `--start-ts` | 1780000000 |
| pid base | 1000 |
| `dev` | 1 |
| comm | `postgres`, `mysqld`, `nginx`, `java`, `python3`, `redis-server`, `tidemarkd`, `cargo`, `rustc`, `node`, `dockerd`, `containerd`, `rsyslogd`, `sshd`, `cron` (each ≤ 15 bytes) |
| unit | NULL when `process_index % 10 == 0`, otherwise `{comm}.service` |
| mix | `splitmix64(seed ^ pass*0xD1B54A32D192ED03 ^ pid*0x94D049BB133111EB ^ slot*0xBF58476D1CE4E5B9)` |
| mix vector | `mix(42, 0, 1000, 0) = 4a947683533f72ef` (815 mod 1000, so that slot does not churn) |
| churn | `mix % 1000 < churn_permille`, including pass 0 |
| path | `{mount}/srv/{comm}.service/pid-{pid}/slot-{slot}/gen-{generation}/app.log`, then ASCII `a` up to the slot's length class. Mount `/` does not add a second slash. v1's only class is 64, which is the core length, so no byte is padding on the longest comm |
| disk | writer `size` and `disk` are `4096 * (pass - opened_at + 1)`. A phantom's `bytes` are the previous writer disk for that generation, or 0 if it was never written |
| inode | `(u64)pid << 40 \| slot << 20 \| generation`, checked to fit in an SQLite integer |

On churn, the phantom row keeps the old generation and the writer row uses the incremented generation.

Section `append_only` is that first week with no `compact` and no deletes. Freelist 0 here means nothing was freed, not that deleted pages were reused.

Section `retention` then calls `TimeSeries::compact`. `now` is `start_ts + duration_secs` (1780604800). The hot window is 518400 seconds, so the cutoff is 1780086400 (the first day). See Calibration for why that window is 6 days rather than the shipped 7. `compact_deleted` must be nonzero. One more day (1440 passes) is inserted on the same generator state, then the file is checkpointed again. That second section is the delete-and-reuse picture. It is not part of `cargo test`.

## Command

Dev profile. SQLite page layout does not depend on Rust optimization level. Bundled SQLite is rusqlite 0.38.0 / libsqlite3-sys 0.36.0 (`SELECT sqlite_version()` = 3.51.1). A rusqlite bump needs a new baseline.

Measured from the dev-profile `xtask` binary in this change, Linux 6.12.94+ x86_64, 4 CPUs, 16 GiB RAM. This is not an isolated benchmark host. The binary took 8.4 s. That is not a result. The previous profile (16×5, paths 96–256) took about 33 s on a dev build when it was rerun for review.

```sh
cargo run --locked -p xtask -- ts-size --profile v1 --out /tmp/tidemark-ts-size-v1
```

The output directory must not already contain `timeseries.db`.

## Append-only results

`page_size` 4096, `page_count` 51689, freelist 0, main file 211,718,144 bytes, wal 0, checkpoint `(0, 0, 0)`, `journal_mode` wal, `auto_vacuum` 0. The writer connection uses `synchronous=NORMAL`; that pragma is not stored in the file.

| dbstat name | Bytes | Rows |
|---|---:|---:|
| `writer_samples` | 165,539,840 | 1,290,240 |
| `writer_ts` | 19,206,144 | |
| `phantom_samples` | 24,068,096 | 193,929 |
| `phantom_ts` | 2,859,008 | |
| eleven other roots | 4,096 each | 0 |

The other roots are `alerts`, `dir_series`, `fs_samples`, `hot_minutes`, `kv`, `lvm_samples`, `scans`, `scans_root_time`, `sqlite_autoindex_kv_1`, `sqlite_autoindex_scans_1`, and `sqlite_schema`. Their 45,056 bytes plus the four sample objects equal the main file. 1,290,240 writer rows is `16 * 8 * 10080`. `writer_ts / writer_samples` is 11.6%.

Every path in the whole run (append plus the reuse day) is 64 bytes: 1,696,274 rows.

## Retention results

`compact_deleted` 211,972, `compact_inserted` 0. The delete is 184,320 writer rows (`128 * 1440`) and 27,652 phantom rows (the first day). No other table had rows.

| | After delete | After one reuse day |
|---|---:|---:|
| `page_count` | 51,689 | 51,725 |
| freelist pages | 7,352 | 0 |
| main file bytes | 211,718,144 | 211,865,600 |
| `writer_samples` bytes / rows | 141,918,208 / 1,105,920 | 165,572,608 / 1,290,240 |
| `writer_ts` bytes | 16,502,784 | 19,251,200 |
| `phantom_samples` bytes / rows | 20,656,128 / 166,277 | 24,104,960 / 194,062 |
| `phantom_ts` bytes | 2,482,176 | 2,891,776 |

Reuse brought the freelist back to 0 and added 36 pages (147,456 bytes) over the append-only file.

## CI

`cargo test -p xtask --bin xtask -- ts_size` runs a 4-process, 3-file, 5-pass, seed-42, churn-250, 64-byte profile twice. Append-only is exactly `page_count` 17 (69,632 bytes): `writer_samples` 12,288 (3 pages), `writer_ts` 4,096, `phantom_samples` 4,096, `phantom_ts` 4,096, and every other dbstat name 4,096. Writer rows are 60 and phantom rows are 14. Those counts are the v1 CI baseline. A rusqlite bump that changes packing has to change the constants with it. The same test checks the dbstat identity, byte-for-byte agreement aside from `shm`, that compact deletes rows, and that a busy checkpoint is refused. After the delete, `page_count` is still 17. After the two reuse passes it is 18, with `writer_samples` 16,384 bytes and the other three sample objects still one page each.

## Comparing a later PR

Rerun the command above on the same SQLite build. Compare `append_only` and `retention` page_count, freelist, main-file length, the four sample object byte counts, and the row counts. Leave out `shm_bytes` and elapsed time. The host 223 MiB file is context. Read the ratios in Calibration before treating a harness percentage as a host percentage.
