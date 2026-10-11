CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Required keys: schema_version, tool_version, scan_id, root (hex of raw bytes), root_display,
-- mode (walk|bulk), mode_reason, status (complete|partial), degraded (0|1), exclude (JSON list), link_instances (all|none), started_at, finished_at (unix s),
-- host, kernel, os_release, fs_uuid, fs_dev, fs_blocksize, threads, throttle_profile,
-- dirs, files, inodes, errors, races, skipped, noatime_fallbacks, phase_ms (JSON), gate_stats (JSON)

CREATE TABLE dirs (
  id             INTEGER PRIMARY KEY,   -- preorder index; root = 0
  parent_id      INTEGER,               -- NULL for root
  subtree_end    INTEGER NOT NULL,      -- last preorder id in this subtree (inclusive)
  depth          INTEGER NOT NULL,      -- root = 0
  name           BLOB    NOT NULL,      -- last component; root row: full root path
  path           BLOB    NOT NULL,      -- full absolute path (raw bytes)
  path_hash      INTEGER NOT NULL,      -- xxh3_64(path) reinterpreted as i64
  ino            INTEGER NOT NULL,
  mtime          INTEGER NOT NULL,
  dir_disk       INTEGER NOT NULL,      -- this directory inode's own allocated bytes
  dir_apparent   INTEGER NOT NULL,      -- this directory inode's st_size
  projid         INTEGER,               -- bulk mode only
  entries        INTEGER NOT NULL,      -- direct entries
  errors         INTEGER NOT NULL,      -- errors in this directory itself
  own_files      INTEGER NOT NULL,      -- direct non-directory entries (link instances)
  own_symlinks   INTEGER NOT NULL,
  own_charged    INTEGER NOT NULL,      -- dir_disk + single-found inodes here + link sets whose LCA is here (03 §4)
  own_apparent   INTEGER NOT NULL,      -- same composition using apparent sizes (dir_apparent + …)
  t_dirs         INTEGER NOT NULL,
  t_files        INTEGER NOT NULL,
  t_inodes       INTEGER NOT NULL,
  t_disk         INTEGER NOT NULL,
  t_apparent     INTEGER NOT NULL,
  t_charged      INTEGER NOT NULL,
  t_reclaimable  INTEGER NOT NULL,
  t_errors       INTEGER NOT NULL,
  age0 INTEGER NOT NULL, age1 INTEGER NOT NULL, age2 INTEGER NOT NULL,
  age3 INTEGER NOT NULL, age4 INTEGER NOT NULL, age5 INTEGER NOT NULL,
  newest_mtime   INTEGER NOT NULL,
  max_entries    INTEGER NOT NULL
);

CREATE TABLE files (                    -- notable single-link files, plus one representative per notable link set
  id       INTEGER PRIMARY KEY,          -- zero-based notable-file row identifier (u32)
  dir_id   INTEGER NOT NULL,              -- representative: the link instance with the smallest (dir_id, name)
  name     BLOB,                        -- NULL if bulk-mode name resolution lost a race
  ino      INTEGER NOT NULL,
  disk     INTEGER NOT NULL,
  apparent INTEGER NOT NULL,
  mtime    INTEGER NOT NULL,
  nlink    INTEGER NOT NULL
);

CREATE TABLE link_sets (
  ino         INTEGER PRIMARY KEY,
  nlink       INTEGER NOT NULL,
  found       INTEGER NOT NULL,
  complete    INTEGER NOT NULL,         -- found >= nlink
  disk        INTEGER NOT NULL,
  apparent    INTEGER NOT NULL,
  mtime       INTEGER NOT NULL,
  lca_dir_id  INTEGER NOT NULL
);
CREATE TABLE link_instances (
  ino    INTEGER NOT NULL,
  dir_id INTEGER NOT NULL,
  name   BLOB                           -- NULL in bulk mode (resolved live on demand)
);

CREATE TABLE skipped     (dir_id INTEGER NOT NULL, name BLOB NOT NULL, reason TEXT NOT NULL);   -- mount|other_fs|excluded
CREATE TABLE errors      (dir_id INTEGER, path BLOB NOT NULL, op TEXT NOT NULL, errno INTEGER NOT NULL);
CREATE TABLE annotations (dir_id INTEGER NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL);

-- Created AFTER bulk load (see §2):
CREATE UNIQUE INDEX dirs_path      ON dirs(path_hash, path);
CREATE INDEX        dirs_parent    ON dirs(parent_id);
CREATE INDEX        files_disk     ON files(disk DESC);
CREATE INDEX        files_dir      ON files(dir_id);
CREATE INDEX        li_ino         ON link_instances(ino);
CREATE INDEX        li_dir         ON link_instances(dir_id);
CREATE INDEX        ls_lca         ON link_sets(lca_dir_id);
