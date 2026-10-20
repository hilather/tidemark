# timeseries.db size: v1 baseline

PR A records the current `TimeSeries` write path. It does not change the schema, the insert statements, compaction, or procscan. Later PRs rerun the same command and compare the fields below.

## Host observation

These figures are from a production `timeseries.db`. This harness did not open that file.

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

The four named objects are about 214 MB. The rest of the file is other dbstat names and rounding in the production report.

## What the harness measures

`cargo xtask ts-size` builds synthetic writer and phantom rows and writes them with `TimeSeries::insert_phantom` and `TimeSeries::insert_writers`. Each pass is one phantom transaction, then one writer transaction, chunked at `tidemarkd::services::tswriter::MAX_BATCH_ROWS` (20,000). Empty passes skip the empty call. v1 has 80 rows per pass, so a chunk boundary is never reached.

Rows are ordered by `(pid, slot)`. That is not procscan's growth order or `(dev, ino)` order. One mount is used. Indexes are only `phantom_ts(ts)` and `writer_ts(ts)`, so path length changes the table and barely changes the index.

After the writer connection is dropped, a second connection runs `PRAGMA wal_checkpoint(TRUNCATE)` and refuses the report unless the result is not busy, every WAL frame was copied, the `-wal` file is missing or empty, `main_file_len == page_count * page_size`, and `SUM(dbstat.pgsize) + freelist_count * page_size == page_count * page_size` (one reserved lock page is allowed only past 1 GiB). dbstat bytes are allocated pages, not payload. `shm` is not part of the comparison.

The command writes `OUT/timeseries.db` and `OUT/report.json`. It refuses an existing database. `--out` must not contain `..`; the directory is created mode `0700` and must pass the state-directory ancestry checks.

### Generator

| Parameter | v1 |
|---|---|
| `--profile` | `v1` |
| `--processes` | 16 |
| `--files-per-process` | 5 (80 open files per pass; every length class occurs) |
| `--interval-secs` | 60 |
| `--duration-secs` | 604800 (10,080 passes, seven days) |
| `--seed` | 42 |
| `--churn-permille` | 200 |
| `--path-bytes` | 96,128,160,192,256 |
| `--mount` | `/data` |
| `--start-ts` | 1780000000 |
| pid base | 1000 |
| `dev` | 1 |
| comm | `postgres`, `mysqld`, `nginx`, `java`, `python3`, `redis-server`, `tidemarkd`, `cargo`, `rustc`, `node`, `dockerd`, `containerd`, `rsyslogd`, `sshd`, `cron` (each ≤ 15 bytes) |
| unit | NULL when `process_index % 10 == 0`, otherwise `{comm}.service` |
| mix | `splitmix64(seed ^ pass*0xD1B54A32D192ED03 ^ pid*0x94D049BB133111EB ^ slot*0xBF58476D1CE4E5B9)` |
| mix vector | `mix(42, 0, 1000, 0) = 4a947683533f72ef` (815 mod 1000, so that slot does not churn) |
| churn | `mix % 1000 < churn_permille`, including pass 0 |
| path | `{mount}/srv/{comm}.service/pid-{pid}/slot-{slot}/gen-{generation}/app.log`, then ASCII `a` up to the slot's length class. Mount `/` does not add a second slash |
| disk | writer `size` and `disk` are `4096 * (pass - opened_at + 1)`. A phantom's `bytes` are the previous writer disk for that generation, or 0 if it was never written |
| inode | `(u64)pid << 40 \| slot << 20 \| generation`, checked to fit in an SQLite integer |

On churn, the phantom row keeps the old generation and the writer row uses the incremented generation.

Section `append_only` is that first week with no `compact` and no deletes. Freelist 0 here means nothing was freed, not that deleted pages were reused.

Section `retention` then calls `TimeSeries::compact`. `now` is `start_ts + duration_secs` (1780604800). The hot window is 518400 seconds, so the cutoff is 1780086400 (the first day). `compact_deleted` must be nonzero. One more day (1440 passes) is inserted on the same generator state, then the file is checkpointed again. That second section is the delete-and-reuse picture. It is not part of `cargo test`.

## Command

Dev profile. SQLite page layout does not depend on Rust optimization level. Bundled SQLite is rusqlite 0.38.0 / libsqlite3-sys 0.36.0 (`SELECT sqlite_version()` = 3.51.1). A rusqlite bump needs a new baseline.

Measured on this change's dev `xtask`, Linux 6.12.94+ x86_64, 4 CPUs, 16 GiB RAM. This is not an isolated benchmark host. Wall time was a few seconds and is not a result.

```sh
cargo run --locked -p xtask -- ts-size --profile v1 --out /tmp/tidemark-ts-size-v1
```

The output directory must not already contain `timeseries.db`.

## Append-only results

`page_size` 4096, `page_count` 59188, freelist 0, main file 242,434,048 bytes, wal 0, checkpoint `(0, 0, 0)`, `journal_mode` wal, `auto_vacuum` 0. The writer connection uses `synchronous=NORMAL`; that pragma is not stored in the file.

| dbstat name | Bytes | Rows |
|---|---:|---:|
| `writer_samples` | 190,365,696 | 806,400 |
| `writer_ts` | 11,993,088 | |
| `phantom_samples` | 37,650,432 | 161,659 |
| `phantom_ts` | 2,379,776 | |
| eleven other roots | 4,096 each | 0 |

The other roots are `alerts`, `dir_series`, `fs_samples`, `hot_minutes`, `kv`, `lvm_samples`, `scans`, `scans_root_time`, `sqlite_autoindex_kv_1`, `sqlite_autoindex_scans_1`, and `sqlite_schema`. Their 45,056 bytes plus the four sample objects equal the main file. 806,400 writer rows is `16 * 5 * 10080`.

Path-length row counts for the whole run (append plus the reuse day), not the append-only section alone: 96 bytes 221,027; 128 bytes 221,271; 160 bytes 221,332; 192 bytes 221,471; 256 bytes 221,438.

## Retention results

`compact_deleted` 138,110, `compact_inserted` 0. The delete is 115,200 writer rows and 22,910 phantom rows (the first day). No other table had rows.

| | After delete | After one reuse day |
|---|---:|---:|
| `page_count` | 59,188 | 59,241 |
| freelist pages | 8,412 | 0 |
| main file bytes | 242,434,048 | 242,651,136 |
| `writer_samples` bytes / rows | 163,201,024 / 691,200 | 190,398,464 / 806,400 |
| `writer_ts` bytes | 10,317,824 | 12,034,048 |
| `phantom_samples` bytes / rows | 32,346,112 / 138,749 | 37,761,024 / 162,029 |
| `phantom_ts` bytes | 2,068,480 | 2,412,544 |

Reuse brought the freelist back to 0 and added 53 pages (217,088 bytes) over the append-only file.

## CI

`cargo test -p xtask --bin xtask -- ts_size` runs a 4-process, 3-file, 5-pass, seed-42, churn-250 profile twice. It checks page size 4096, freelist 0, wal 0, writer rows 60, phantom rows 14, `writer_samples` at least two pages, both indexes at least one page, every other table empty, the dbstat identity, and byte-for-byte agreement aside from `shm`. The main file must stay under 8 MiB. That ceiling is a backstop, not the v1 number. The same test binary also checks that the ci profile's compact deletes rows and that a busy checkpoint is refused.

## Comparing a later PR

Rerun the command above on the same SQLite build. Compare `append_only` and `retention` page_count, freelist, main-file length, the four sample object byte counts, and the row counts. Leave out `shm_bytes` and elapsed time. The host 223 MiB file is context, not this command's expected total.
