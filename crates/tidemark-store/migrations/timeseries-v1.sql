CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE scans (
  scan_id TEXT PRIMARY KEY, root BLOB NOT NULL, root_slug TEXT NOT NULL, mode TEXT NOT NULL,
  started_at INTEGER NOT NULL, finished_at INTEGER, status TEXT NOT NULL,   -- running|complete|partial|failed
  degraded INTEGER NOT NULL DEFAULT 0, file TEXT, t_disk INTEGER, t_inodes INTEGER, t_dirs INTEGER,
  errors INTEGER, duration_ms INTEGER, tool_version TEXT NOT NULL);
CREATE INDEX scans_root_time ON scans(root_slug, started_at);

-- res: 0 = raw (sample interval), 1 = 5-minute, 2 = 1-hour. Gauges keep the last value in the bucket.
CREATE TABLE fs_samples (mount TEXT NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  size INTEGER NOT NULL, used INTEGER NOT NULL, avail INTEGER NOT NULL,
  inodes INTEGER NOT NULL, inodes_used INTEGER NOT NULL,
  PRIMARY KEY (mount, res, ts)) WITHOUT ROWID;

CREATE TABLE lvm_samples (vg TEXT NOT NULL, lv TEXT NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  lv_size INTEGER NOT NULL, vg_free INTEGER NOT NULL,
  data_pct REAL, meta_pct REAL,                                   -- thin pools / thin LVs only
  PRIMARY KEY (vg, lv, res, ts)) WITHOUT ROWID;

CREATE TABLE dir_series (root_slug TEXT NOT NULL, path_hash INTEGER NOT NULL, ts INTEGER NOT NULL,
  scan_id TEXT NOT NULL, path BLOB NOT NULL,
  t_disk INTEGER NOT NULL, t_reclaimable INTEGER NOT NULL, t_files INTEGER NOT NULL,
  PRIMARY KEY (root_slug, path_hash, ts)) WITHOUT ROWID;          -- dirs with depth <= series_depth, max 20,000 rows per scan (largest first)

CREATE TABLE hot_minutes (mount TEXT NOT NULL, scope TEXT NOT NULL, key BLOB NOT NULL, ts INTEGER NOT NULL,
  events INTEGER NOT NULL, files INTEGER NOT NULL, bytes_delta INTEGER NOT NULL, lossy INTEGER NOT NULL,
  PRIMARY KEY (mount, scope, key, ts)) WITHOUT ROWID;              -- scope: dir|unit|comm; top-K per minute only

CREATE TABLE phantom_samples (ts INTEGER NOT NULL, mount TEXT NOT NULL, pid INTEGER NOT NULL, comm TEXT NOT NULL, unit TEXT,
  path BLOB NOT NULL, dev INTEGER NOT NULL, ino INTEGER NOT NULL, bytes INTEGER NOT NULL);
CREATE TABLE writer_samples  (ts INTEGER NOT NULL, mount TEXT NOT NULL, pid INTEGER NOT NULL, comm TEXT NOT NULL, unit TEXT,
  path BLOB NOT NULL, dev INTEGER NOT NULL, ino INTEGER NOT NULL, size INTEGER NOT NULL, disk INTEGER NOT NULL);
CREATE INDEX phantom_ts ON phantom_samples(ts);
CREATE INDEX writer_ts  ON writer_samples(ts);

CREATE TABLE pg_samples (kind TEXT NOT NULL, name TEXT NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  value INTEGER NOT NULL,
  PRIMARY KEY (kind, name, res, ts)) WITHOUT ROWID;
-- kind: database | relation | wal_dir | slot_retained | temp_bytes_total | archiver_failed_total | dead_tuples

CREATE TABLE alerts (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, rule TEXT NOT NULL, subject TEXT NOT NULL,
  state TEXT NOT NULL, value REAL, message TEXT NOT NULL);         -- state: firing|resolved|renotify
