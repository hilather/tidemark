# 15 — Snapshot format v2 and cold snapshots

Status: approved by the owner 2026-10-17 (implementation plan "Path A"). Rationale and measurements:
the coordinator's evaluation (summarised in §0). This document is the contract the SNAP-* tasks build to.
`05-storage.md` §1 remains the v1 contract; readers keep v1 support for one retention cycle (§9).

## 0. Why

Measured on a replica of a 23,490-directory tree: v1 costs ~264 B per directory, more than half of it the
full `path` stored twice (row + `dirs_path` index) although `parent_id` + name-sorted preorder ids +
`subtree_end` define it. Columnar chunks (delta/varint + zstd) measured ~32 B per directory and ~10 B per
notable file. A whole v1 file compresses ~4.8× with zstd-19. Goal: the smallest footprint that keeps the
TUI and CLI latency budget (§8).

## 1. Two independent features

- **Cold snapshots (Tier 0):** retention compresses complete snapshots outside the `recent` set into a
  `.db.zst` file (§2). Works for v1 and v2 files. Lands first.
- **Format v2 (Tier 2):** `PRAGMA user_version = 2` snapshot files whose bulk tables are columnar zstd
  chunks (§3–§6). Written by new scans once the v2 writer lands; v1 files remain readable.

## 2. Cold file format (`snapshots/<root-slug>/<scan_id>.db.zst`)

```
offset 0   8 bytes   magic  b"TMCOLD01"
offset 8   u64 LE    uncompressed length of the SQLite file (bytes)
offset 16  u32 LE    meta JSON length M
offset 20  M bytes   header JSON: {"user_version":N,"meta":{<every meta key/value of the snapshot>},"root":<root DirRow as serde JSON>}
offset 20+M          one zstd frame containing the complete SQLite file
```

- Compression level from config `[retention.snapshots] cold_level` (default 9, range 1..=19);
  `cold_after_recent = true` (default) enables it. `recent` snapshots and each root's newest complete
  snapshot are never cold.
- Publication order (crash-safe): write `<scan_id>.db.zst.tmp` exclusively (0600, trust checks as other
  leaves), fsync, rename NOREPLACE to `.db.zst`, fsync directory, durably update the catalog row's `file` (`synchronous=FULL`), then
  unlink the `.db` and fsync the directory. A crash leaves at most both files; startup cleanup removes the
  `.db` only after streaming byte-for-byte equivalence with `.db.zst` is proved, and removes stray
  `.db.zst.tmp`. A failed comparison keeps the hot file and removes the bad cold copy. Cleanup also
  repairs a stale hot catalog pointer when only a valid cold file remains.
- Catalog: `scans.file` points at whichever file exists; `rebuild_from_files` reads cold metadata from
  the header JSON without inflating for cold-only snapshots; when both forms exist, rebuilding
  prefers the readable hot file and leaves pair resolution to cleanup.
- Opening a cold snapshot inflates it to `snapshots/<root-slug>/.inflate/<scan_id>.<pid>.<n>.db` (private
  0700 directory inside state_dir, trust checks apply), opens that file `immutable=1`, and unlinks it
  when the `Snapshot` is dropped. The space guard accounts the uncompressed length before inflating and
  refuses (NoSpace) rather than fill the filesystem. Startup and the scan-lock holder sweep stale
  `.inflate/*` leaves.
- Concurrent cold opens each check the space guard independently and may jointly overshoot the
  free-space floor; no shared reservations are made.
- Space-guard estimates (`previous × 1.25`) use the uncompressed length from the header for cold files.
- `snapshots list` shows a `cold` marker and both sizes; `prune`, `rebuild-catalog`, corruption recovery
  and retention handle both forms. Uncompressed `.db` files are never rewritten in place.

## 3. v2 file layout

```sql
PRAGMA user_version = 2;
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- v1 keys + plain_depth, hash_width, chunk_dirs, chunk_files
CREATE TABLE dir_chunks  (first_id INTEGER PRIMARY KEY, last_id INTEGER NOT NULL, data BLOB NOT NULL);
CREATE TABLE file_chunks (first_row INTEGER PRIMARY KEY, last_row INTEGER NOT NULL,
                          first_dir INTEGER NOT NULL, last_dir INTEGER NOT NULL, data BLOB NOT NULL);
CREATE TABLE li_chunks   (first_row INTEGER PRIMARY KEY, last_row INTEGER NOT NULL,
                          first_ino INTEGER NOT NULL, last_ino INTEGER NOT NULL, data BLOB NOT NULL);
CREATE TABLE dir_lookup  (hash INTEGER NOT NULL, id INTEGER NOT NULL, PRIMARY KEY (hash, id)) WITHOUT ROWID;
CREATE TABLE shallow_dirs (<every v1 dirs column except path and path_hash>);  -- copies of dirs with depth <= plain_depth
CREATE INDEX shallow_parent ON shallow_dirs(parent_id);
CREATE TABLE link_sets  (<as v1>);  CREATE INDEX ls_lca ON link_sets(lca_dir_id);
CREATE TABLE skipped    (<as v1>);  CREATE TABLE errors (<as v1>);  CREATE TABLE annotations (<as v1>);
```

- `dirs.path` does not exist. A path is the parent chain of names; readers reconstruct it.
  `DirRow.path` and `path_hash` are still filled by the reader (API unchanged).
- `dir_lookup.hash` is xxh3_64 of the raw path, truncated to `hash_width` bits (32 or 64, chosen by the
  benchmark and recorded in meta). `dir_by_path` probes the hash and verifies candidates by reconstruction.
- `shallow_dirs` duplicates the full rows of directories at `depth <= plain_depth` (meta key; default =
  the root's `series_depth`, at least 2) so root listings, `top`/`wide` at shallow depth and the daemon's
  growth series read plain SQL rows. Chunks contain **all** directories (single source for range scans).
- No `files_*`, `dirs_*` or `li_*` secondary indexes; chunk tables are keyed by their first id/row and
  located by binary search on that key.

## 4. Chunk codec (`crates/tidemark-store/src/chunk.rs`)

Every chunk `data` blob: `u8 codec_version (=1)`, `varint column_count`, `column_count × varint
uncompressed column length`, then one zstd frame of the concatenated columns. Varints are LEB128;
signed deltas are zig-zag. Level 3 by default (constant, configurable for the writer).

- **Dir chunk:** `chunk_dirs` (default 4096) consecutive preorder ids starting at `first_id`. Columns, in
  order: `id − parent_id` (root writes 0 and is identified by `id = 0`), `subtree_end − id`, `depth`, name
  (varint length + raw bytes; the root row's name is its full root path), `ino` zig-zag delta from the
  previous row in the chunk (first row from 0), `mtime` zig-zag delta likewise, `newest_mtime − mtime`
  zig-zag, `projid` (0 = NULL, else value + 1), then the counters `dir_disk, dir_apparent, entries,
  errors, own_files, own_symlinks, own_charged, own_apparent, t_dirs, t_files, t_inodes, t_disk,
  t_apparent, t_charged, t_reclaimable, t_errors, age0..age5, max_entries` as varints, then `file_lo −
  previous file_lo` (first row absolute) and `file_count`.
- **File chunk:** `chunk_files` (default 8192) notable-file rows in `(dir_id, name)` order (row number =
  `files.id` in v2). Columns: `dir_id` delta, name presence+length+bytes (`0` = NULL, else len + 1), `ino`
  zig-zag delta, `disk`, `apparent`, `mtime` zig-zag delta, `nlink`.
- **Link-instance chunk:** 8192 rows in `(ino, dir_id, name)` order. Columns: `ino` delta, `dir_id`
  zig-zag delta, name presence+length+bytes.
- Decoders validate every length and count, reject trailing bytes, and use the same checked integer
  conversions as v1 (§1 of `05`). A corrupt chunk is `StoreError::Invalid`, never a panic.
- The API is pure (no SQLite): `encode_dirs(&[DirRow2]) -> Vec<u8>`, `decode_dirs(&[u8], first_id) ->
  Vec<DirRow2>` and the same for files and link instances, plus column-projection decode for the
  hot sort keys (`t_disk`, `t_reclaimable`, `t_files`, `newest_mtime`, `entries`) so metric sorts avoid
  materialising whole rows.

## 5. Queries in v2

- Subtree `[lo, hi]` → the chunk range covering `lo..=hi` (binary search on `first_id`).
- Children of `d`: shallow table when `d.depth < plain_depth`; otherwise sibling hopping inside the
  decoded subtree range: first child `d.id + 1` if `d.subtree_end > d.id`, next sibling
  `child.subtree_end + 1`, stop past `d.subtree_end`. Name order is preorder order; metric sorts
  materialise only the child set.
- Notable files under `[lo, hi]`: file chunks whose `[first_dir, last_dir]` overlaps, filtered and
  top-N by heap. ncdu paging per directory uses `file_lo`/`file_count`.
- `ORDER BY path` everywhere becomes preorder order (`ORDER BY id`).
- Link sets: plain table as v1; instances for an inode by binary search on `li_chunks.first_ino`.

## 6. Diff as a tree walk

`diff` is implemented on the `Snapshot` API, not by ATTACH: walk both trees from their roots, merge each
directory's children by name bytes (both are name-sorted), recurse into matched pairs, emit added/removed
subtrees whole. O(n + m), O(depth) state, no hashing, works for any v1/v2 pair. Output (text and JSON,
ordering, limits, notices) must be byte-identical to the v1 implementation on the existing fixtures.

## 7. `tidemark snapshots export --sqlite <scan> <out.db>`

Materialises a v1-schema database (with `path`) from any snapshot, for ad-hoc `sqlite3` analysis and
off-box copies. Output path must not exist; 0600.

## 8. Latency gate (Tier 2 proceeds only if met)

Benchmark (`cargo run --release -p tidemark-store --example snapbench`) on a synthetic snapshot with
2,000,000 directories (fan-out and depth like a large data root: median depth 7, a few directories with
100k+ children), 200,000 notable files and 500,000 link instances. Measure cold-cache (fresh `Snapshot`
open) and warm latency for: root `children_page` by `t_disk` (first page of 50); `children_page` of a
100k-child directory by `t_disk`; `top` at depth ≤ 2 and ≤ 4; notable top-N under root and under a
1,000-directory subtree; `dir_by_path` for a depth-12 path; `diff` against a copy with 1% of directories
changed; ncdu export. Acceptance per query: v2 ≤ max(1.25 × v1, 100 ms). Also report file sizes and peak
RSS. A miss stops Tier 2 integration until the coordinator reports to the owner.

## 9. Compatibility

- Readers support `user_version` 1 and 2 (05 §9 "never migrated" still holds). v1 support is removed no
  earlier than one full default retention cycle (8 weeks) after the v2 writer ships.
- `SnapshotMeta.schema_version` becomes 2 for new scans; `writer.rs` accepts only the version it writes.
- Space guard first-scan estimate: `dirs_estimate × 64 B` for v2.
- The public row types (`DirRow`, `FileRow`, …) and the `Snapshot` method signatures stay; `files.id`
  semantics change to the clustered row number in v2 (documented in SCR form).
