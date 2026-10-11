# 14 — PostgreSQL tracking (deferred)

**Not part of v1.** The owner decided on 2026-10-11 to remove PostgreSQL-specific tracking to keep Tidemark simple (ADR-019). It may return as an add-on. This document preserves the former design as reference, not implementation instructions for the active waves.

## 1. Why it was cut

Database connections, credentials, version-adaptive queries, dedicated storage, alerts and daemon wiring expand the tool beyond disk-usage intelligence. No version has been released, so there is no migration or compatibility mode. The v1 schema, config and API simply omit these items; `[postgres]` is an unknown table and is rejected, as are PostgreSQL-only keys and alert kinds.

The adaptive throttle, scan windows, one scan at a time and `O_NOATIME` remain. PostgreSQL shares the disks and is a workload to protect: R2 and the Wave 7 `pgbench` latency gate remain. A data directory on `/data` is scanned like any other directory. Operators can use generic scan and watcher exclusions. Snapshot annotations remain generic, without automatic `pg.*` labels.

## 2. Former collector design (reference)


### 2.1 Access

```sql
CREATE ROLE tidemark LOGIN;
GRANT pg_monitor TO tidemark;      -- PG10+: read all stats/settings, pg_ls_waldir()
```

The daemon runs as root and connects over the Unix socket. Either:

- **peer auth with an ident map** (`pg_ident.conf`: `tidemark  root  tidemark`; `pg_hba.conf`: `local all tidemark peer map=tidemark`), or
- **a password file** at `/etc/tidemark/pg.pass` (0600), used via `passfile=` in the DSN.

### 2.2 Queries (version-adaptive; probe `server_version_num` first)

| Every | Query | Notes |
|-------|-------|-------|
| 5 min | `SELECT datname, pg_database_size(oid) FROM pg_database WHERE datallowconn AND NOT datistemplate` | `kind=database` |
| 5 min | `SELECT COALESCE(sum(size),0) FROM pg_ls_waldir()` | `kind=wal_dir`; PG ≥ 10 |
| 5 min | `SELECT slot_name, slot_type, active, pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn) FROM pg_replication_slots` | `kind=slot_retained`; skip when `pg_is_in_recovery()`; add `wal_status` on PG ≥ 13 |
| 5 min | `SELECT archived_count, failed_count, last_failed_time, last_archived_time FROM pg_stat_archiver` | `kind=archiver_failed_total`; a failing archiver makes WAL pile up |
| 5 min | `SELECT datname, temp_bytes, temp_files FROM pg_stat_database WHERE datname IS NOT NULL` | `kind=temp_bytes_total` (cumulative counter, so take the rate) |
| 1 h | Per database: `SELECT n.nspname, c.relname, c.relkind, pg_total_relation_size(c.oid) AS b FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r','m') ORDER BY b DESC NULLS LAST LIMIT $top` | `kind=relation`, `name = <db>.<schema>.<relation>` (unique across databases; total size already includes TOAST and indexes, so TOAST tables aren't listed separately); one connection per database, sequential, with `statement_timeout = 30s` |
| 1 h | Per database: `SELECT schemaname, relname, n_live_tup, n_dead_tup, last_autovacuum FROM pg_stat_user_tables ORDER BY n_dead_tup DESC LIMIT 20` | `kind=dead_tuples`, a bloat hint |
| On start + daily | `SHOW data_directory`; `SELECT spcname, pg_tablespace_location(oid) FROM pg_tablespace`; `SELECT oid, datname FROM pg_database` | Path labels: annotate `<data_directory>`, `base/<oid>` → `pg.database=<name>`, tablespace dirs, `pg_wal` |

Every query sets `application_name=tidemark` and a statement timeout. Failures back off 1, 2, 4 … up to 30 minutes, and the status shows "pg: unreachable since …".

## 3. Former configuration and interfaces (reference)

```toml
[postgres]
enabled            = false
dsn                = "host=/var/run/postgresql user=tidemark dbname=postgres application_name=tidemark"
interval           = "5m"
relations_interval = "1h"
top_relations      = 20
statement_timeout  = "30s"

```

The former `watch.exclude_postgres` setting automatically excluded the discovered data directory; procscan added its `base/` and `global/` paths. The former optional password file was `/etc/tidemark/pg.pass` (0600 root:root).

The removed core contracts were `PostgresConfig`, `PgSample`, `PgSlot`, `PgNow` and the `pg` fields in `NowReport`, `AlertInputs` and `NowInputs`. The CLI command `tidemark pg` had `--relations` and `--db NAME`; the `pg` API method accepted `relations?`, `db?` and returned `Option<PgNow>`. Probe appended reachability and missing-privilege recommendations.

## 4. Former alerts, storage and metrics (reference)

| Kind | Parameters | Fires when |
|------|------------|-----------|
| `pg_wal_bytes` | above | WAL directory bytes exceed above |
| `pg_slot_retained` | above | Any replication slot retains more than above |
| `pg_archiver_failing` | window | Archiver failed_count increased within window |

Defaults were `pg_slot_retained > 50 GiB` and `pg_archiver_failing` with a 30-minute window, with the common 5-minute firing / 10-minute resolution hysteresis.

```sql
CREATE TABLE pg_samples (kind TEXT NOT NULL, name TEXT NOT NULL, res INTEGER NOT NULL, ts INTEGER NOT NULL,
  value INTEGER NOT NULL,
  PRIMARY KEY (kind, name, res, ts)) WITHOUT ROWID;
-- kind: database | relation | wal_dir | slot_retained | temp_bytes_total | archiver_failed_total | dead_tuples

```

Retention was 48 h raw, 35 d at 5-minute resolution, and 400 d hourly. Store methods were `insert_pg(&[PgSample])` and `query_pg(kind, name, from, to)`. Metrics were `tidemark_pg_database_bytes{database}`, `tidemark_pg_wal_dir_bytes`, `tidemark_pg_slot_retained_bytes{slot}` and `tidemark_pg_archiver_failed_total`.

## 5. Former wave task and wiring (reference)

### W5-C — Postgres collector (deferred reference)

**Read first:** the former collector design in §2 and interfaces in §3 of this deferred document.

#### Interface

```rust
pub struct PgProbe { pub reachable: bool, pub server_version_num: Option<u32>, pub in_recovery: Option<bool>,
                     pub data_directory: Option<String>, pub missing_privileges: Vec<String>, pub error: Option<String> }
pub fn probe_from_config(cfg: &PostgresConfig) -> Result<PgProbe, PgError>;

/// Our own row type: every column as text (NULL → None), so fakes can build rows freely.
/// (postgres::Row has no public constructor.) Queries cast to text in SQL where needed.
pub type PgRow = Vec<Option<String>>;
pub trait PgConn { fn query(&mut self, sql: &str, params: &[&str]) -> Result<Vec<PgRow>, PgError>; }
pub trait PgConnector: Send { fn connect(&self, dbname: &str) -> Result<Box<dyn PgConn>, PgError>; }
pub struct PgCollector;
impl PgCollector {
    pub fn new(cfg: PostgresConfig) -> Self;                                       // real connections (postgres crate)
    pub fn with_connector(cfg: PostgresConfig, c: Box<dyn PgConnector>) -> Self;  // tests
    pub fn collect_fast(&mut self, now: i64) -> Result<Vec<PgSample>, PgError>;  // 5-minute set
    pub fn collect_relations(&mut self, now: i64) -> Result<Vec<PgSample>, PgError>; // hourly, per database
    pub fn path_labels(&mut self) -> Result<Vec<(RawPath, String, String)>, PgError>; // pg.* annotations
    pub fn latest(&self) -> Option<PgNow>;
    pub fn backoff_until(&self) -> Option<i64>;
}
```

#### Details

- Queries exactly as §2.2, chosen by `server_version_num`:
  - **< 100000:** unsupported; probe reports it.
  - **< 130000:** no `wal_status`.
  - **In recovery:** skip slot retention; report WAL dir only.
- Every session: `SET application_name = 'tidemark'`, `SET statement_timeout = <cfg>`, `SET lock_timeout = '1s'`. Read-only: `SET default_transaction_read_only = on`.
- **Back-off:** 1, 2, 4 … 30 minutes on connection or query failure. `latest()` keeps the last good values and adds `unreachable_since`.
- **`path_labels`:** `data_directory` → `pg.data_directory=true`; `base/<oid>` → `pg.database=<name>`; `pg_wal` → `pg.wal=true`; tablespace locations → `pg.tablespace=<name>`.
- **`missing_privileges` in probe:** test `pg_ls_waldir()`, `pg_database_size` on one database, and reading `data_directory`, and report which fail. The probe text then suggests `GRANT pg_monitor TO tidemark;`.
- **CLI `pg`:** runs `collect_fast` (and `collect_relations` with `--relations`) once and prints:
  - databases by size;
  - WAL directory size;
  - slots with retained bytes and active flag;
  - archiver status;
  - temp-bytes counters;
  - top relations.

#### Tests

- Unit: a fake connector returns canned rows for each version branch (PG 10, 12, 13, 16); back-off schedule; label mapping.
- Integration (VM, or any box with Postgres binaries): `tests/support/pgtmp.rs` runs `initdb` into a temp dir as an unprivileged user, starts on a Unix socket in the temp dir, creates role `tidemark` with `pg_monitor`, creates a database and a table with data plus a physical replication slot → `collect_fast` reports database size > 0, `wal_dir` > 0, and slot retention ≥ 0. A role without `pg_monitor` → probe lists the missing privileges.

#### Acceptance

- [ ] Works against PostgreSQL from the EL8 AppStream module streams installed on the VM (record the versions tested).

---

W0 formerly stubbed `tidemark-pg` and its core contracts; W1 probe composed its report and store methods; W2 consumed path labels as annotations. W4 deploy pre-flight checked access and first boot reported `pg`. W6 started the `pg` service thread, refreshed annotations daily, wired API/direct fallback, exported metrics, packaged credentials and supplied WAL/slot dashboards and `TidemarkPgSlotRetention`. W7 hardened the collector and supplied a WAL-growth runbook. None of this is active v1 work. The workspace `postgres` dependency is removed; the old sync client's internal runtime exception to ADR-005 is unnecessary.

## 6. What adding it back would require

An owner-approved add-on proposal must define scope and revise contracts before implementation: supported server versions and authentication, read-only queries and load bounds, credential handling, bounded sample and relation sets, schema and retention, config/CLI/API/types, alerts and metrics, annotation integration, daemon lifecycle and health semantics, deploy/packaging and operator documentation. Re-review database privileges and security; test version branches, recovery mode, unreachable/slow servers, timeouts and back-off in disposable databases. Establish compatibility and migration policy for whatever version exists then. The reference queries and tasks above are starting points, not frozen future contracts.
