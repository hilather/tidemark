# SNAP-export-sqlite — handoff

## Needs from coordinator

1. `packaging/rpm/rpm.toml` lists one man page per subcommand (e.g. `tidemark-snapshots-rebuild-catalog.1`);
   a new `tidemark-snapshots-export.1` entry is needed there (outside my scope; not touched). Check whether the
   man-page generator/xtask or any packaging test enumerates subcommands.
2. `crates/tidemark-cli/Cargo.toml` has no `rusqlite` dev-dependency, so `tests/snapshots_export.rs` opens the
   export through `tidemark_store::Snapshot` (v1 reader: `dir_by_path` runs `SELECT * FROM dirs WHERE path_hash=? AND path=?`)
   plus a `SQLite format 3` header check rather than a literal `SELECT path FROM dirs`. Add the dev-dep if you want the literal query.
   The store test (`tests/export_sqlite.rs`) does use rusqlite directly and compares every table.
3. Not run here: the privileged build was compiled only (`--features privileged --tests`), nothing privileged was executed.

## Design

- `tidemark_store::export_sqlite(&Snapshot, &Path) -> Result<SqliteExportStats, StoreError>` (`export_sqlite.rs`, re-exported in `lib.rs`).
- Refuses an existing `out` (including a dangling symlink, via `symlink_metadata`) before reading anything (`io::ErrorKind::AlreadyExists`).
- Builds with the existing `SnapshotWriter` (meta cloned with `schema_version = 1`) inside a private 0700 staging dir
  `.tidemark-export-<pid>-<scan_id>` created beside `out` (canonicalised parent), so the writer's 0600/O_EXCL temp, sync,
  indexes and `ANALYZE` are reused. The finished file is moved to `out` with `renameat2 RENAME_NOREPLACE`, the parent is fsynced, and
  a drop guard removes the staging dir on every path (error or success). The result is byte-for-byte the v1 schema with `dirs.path`, `path_hash` and v1 indexes, mode 0600.
- Bounded memory: 10,000-row batches into the writer.
  - dirs: v1 streams `SELECT * FROM dirs ORDER BY id`; v2 calls `Snapshot::dir(id)` for `0..=root.subtree_end` (the public per-id API; `subtree_dirs` is capped at 1M rows and cannot page).
  - files: v1 streams by id (ids preserved); v2 uses `v2_dir_files` per directory, ids assigned sequentially in directory/name order (v2 stores no ids).
  - link sets: plain table in both layouts, keyset-paged by `ino`; instances come from `Snapshot::link_instances(ino)` per set (works for v1 and v2). v1 instances whose inode has no `link_sets` row are copied in a separate pass.
  - skipped / errors / annotations: plain tables, keyset on `rowid`.
- Cold sources: the CLI uses the same `open_snapshot` path as `show`, which inflates `.db.zst` into the private `.inflate/` leaf (space-guarded) and removes it on drop; I did not call `cold::inflate_to` separately because the reader already owns that lifecycle.
- CLI `tidemark snapshots export --sqlite <ID> <OUT> [--root ROOT]`: read-only catalog (`Catalog::open(.., false)`), no daemon check, refuses an output inside `state_dir` (5), unknown id / wrong root (5), `StoreError::Space` or ENOSPC (5). `--sqlite` is required (usage error 2 without it). Text and `--json` (`tidemark.snapshots.v1`) output.

## Assumptions

- "Target exists" exits **1** (runtime), matching the existing `export ncdu -o` behaviour (`AlreadyExists` io error, message contains "already exists"); `docs/plan/08` has no dedicated code for it. Say so if you want 2 or 5.
- Hard-link inode with more than 1,000,000 instances fails the export via `link_instances`' query cap (not realistic; noted).
- The staging dir must satisfy the state-dir trust rules (euid/root-owned, ancestors not group/world-writable unless sticky) because the writer reuses `StateDir`; destinations in untrusted directories are rejected with the writer's error.

## Tests

- `crates/tidemark-store/tests/export_sqlite.rs` (4): v1 and v2 exports of the same tree have identical rows in every table, equal to the v1 source, `user_version` 1, mode 0600, v1 indexes, only the outputs remain; existing/dangling-symlink `out` refused and untouched; undecodable row mid-export leaves an empty destination; cold source equals hot export.
- `crates/tidemark-cli/tests/snapshots_export.rs` (3): scan → export → open + path lookup, 0600, refusal (exit 1, "already exists", file unchanged); JSON, unknown id (5), inside state_dir (5), missing `--sqlite` (2); cold snapshot export leaves `.inflate` empty.
- `tests/surface.rs`: `snapshots export` flags and child count.

## Results (foreground, uid 0, offline)

`cargo fmt --check` ok; clippy `--workspace --all-targets -D warnings` clean; store: `--lib` 58 passed, `export_sqlite` 4, `cold` 25, `writer_v2` 14, `reader_v2` 45, `storage` 33; cli: `snapshots_export` 3, `surface` 13, `snapshots_cold` 3; `--features privileged --tests` builds; `RUSTDOCFLAGS='-D warnings' cargo doc` clean. Not run: `cargo test --workspace`, `tools/ci.sh` (per task).

## Docs

- `docs/operator-guide.md`: "Exporting a snapshot to SQLite".
- `docs/perf/snapbench.md`: v2 `TBD` table replaced by the coordinator's table (pasted as is, plus the measurement note: integration `61abc4c`, host release build, page cache not dropped).

## Counts

Files: 1 new store module, 1 new store test, 1 new CLI test; edits to `lib.rs`, `cmd/snapshots.rs`, `surface.rs`, two docs; no new crates or dependencies.
