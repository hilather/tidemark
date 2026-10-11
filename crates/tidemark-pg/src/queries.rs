//! Owned by: W5-C
//! Pure, text-column SQL for PostgreSQL 10 and later.

/// Database sizes, in deterministic order.
pub const DATABASES: &str = "SELECT datname::text, pg_database_size(oid)::text FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname";
/// Single database size for the privilege probe.
pub const PROBE_DATABASE: &str = "SELECT pg_database_size(oid)::text FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname LIMIT 1";
/// Databases eligible for sequential relation collection.
pub const CONNECT_DATABASES: &str = "SELECT oid::text, datname::text FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname";
/// WAL directory metadata only.
pub const WAL: &str = "SELECT COALESCE(sum(size),0)::text FROM pg_ls_waldir()";
/// Version and recovery status.
pub const VERSION: &str =
    "SELECT current_setting('server_version_num')::text, pg_is_in_recovery()::text";
/// Archiver counters and epoch timestamps.
pub const ARCHIVER: &str = "SELECT archived_count::text, failed_count::text, EXTRACT(EPOCH FROM last_failed_time)::bigint::text, EXTRACT(EPOCH FROM last_archived_time)::bigint::text FROM pg_stat_archiver";
/// Cumulative temporary-file counters.
pub const TEMP: &str = "SELECT datname::text, temp_bytes::text, temp_files::text FROM pg_stat_database WHERE datname IS NOT NULL ORDER BY datname";
/// PGDATA setting.
pub const DATA: &str = "SHOW data_directory";
/// Database OIDs for labels.
pub const DATABASE_LABELS: &str = "SELECT oid::text, datname::text FROM pg_database ORDER BY oid";
/// Tablespace paths (empty built-in locations are omitted by the decoder).
pub const TABLESPACES: &str =
    "SELECT spcname::text, pg_tablespace_location(oid)::text FROM pg_tablespace ORDER BY spcname";
/// Dead tuples with last-autovacuum epoch.
pub const DEAD: &str = "SELECT schemaname::text, relname::text, n_live_tup::text, n_dead_tup::text, EXTRACT(EPOCH FROM last_autovacuum)::bigint::text FROM pg_stat_user_tables ORDER BY n_dead_tup DESC, schemaname, relname LIMIT 20";
/// Slot query selected without issuing a primary-only function on a standby.
pub fn slots(version: u32, recovery: bool) -> Option<&'static str> {
    if recovery || version < 100000 {
        return None;
    }
    Some(if version >= 130000 {
        "SELECT slot_name::text, slot_type::text, active::text, pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn)::bigint::text, wal_status::text FROM pg_replication_slots ORDER BY slot_name"
    } else {
        "SELECT slot_name::text, slot_type::text, active::text, pg_wal_lsn_diff(pg_current_wal_lsn(), restart_lsn)::bigint::text FROM pg_replication_slots ORDER BY slot_name"
    })
}
/// Bounded relation query; limit is rendered from an integer, never user SQL.
pub fn relations(top: usize) -> String {
    format!("SELECT n.nspname::text, c.relname::text, c.relkind::text, pg_total_relation_size(c.oid)::text AS b FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r','m') ORDER BY pg_total_relation_size(c.oid) DESC NULLS LAST, n.nspname, c.relname LIMIT {}", top.min(1000))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn sql_text_is_pinned() {
        let relation = relations(20);
        let statements = [
            DATABASES,
            PROBE_DATABASE,
            CONNECT_DATABASES,
            WAL,
            VERSION,
            ARCHIVER,
            TEMP,
            DATA,
            DATABASE_LABELS,
            TABLESPACES,
            DEAD,
            slots(100000, false).unwrap(),
            slots(130000, false).unwrap(),
            &relation,
        ];
        assert_eq!(
            statements.join("\n") + "\n",
            include_str!("../tests/w5c/sql.txt")
        );
    }
    #[test]
    fn version_branches() {
        for v in [100000, 120000] {
            assert!(!slots(v, false).unwrap().contains("wal_status"));
        }
        for v in [130000, 160000] {
            assert!(slots(v, false).unwrap().contains("wal_status::text"));
        }
        assert!(slots(160000, true).is_none());
        assert!(slots(90600, false).is_none());
        assert!(relations(usize::MAX).ends_with("LIMIT 1000"));
        assert_eq!(
            WAL,
            "SELECT COALESCE(sum(size),0)::text FROM pg_ls_waldir()"
        );
    }
}
