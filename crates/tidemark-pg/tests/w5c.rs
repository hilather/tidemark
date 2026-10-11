//! Owned by: W5-C
//! Pending-VM PostgreSQL integration coverage, without host service changes.
#![cfg(feature = "privileged")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "support/pgtmp.rs"]
mod pgtmp;
use tidemark_core::config::PostgresConfig;
use tidemark_pg::{probe_from_config, PgCollector};

#[test]
fn postgres_monitor_collects_sizes_slots_relations_and_labels() {
    let pg = pgtmp::PgTmp::start();
    let mut admin = pg.admin();
    admin
        .batch_execute("CREATE ROLE tidemark LOGIN; GRANT pg_monitor TO tidemark")
        .unwrap();
    admin.batch_execute("CREATE DATABASE measured").unwrap();
    let mut data_config: postgres::Config = pg.dsn("postgres").parse().unwrap();
    data_config.dbname("measured");
    data_config.connect(postgres::NoTls).unwrap().batch_execute("CREATE TABLE public.measured AS SELECT n, repeat('x',100) AS payload FROM generate_series(1,1000) n").unwrap();
    admin
        .query(
            "SELECT pg_create_physical_replication_slot('tidemark_test',true)",
            &[],
        )
        .unwrap();
    let cfg = PostgresConfig {
        dsn: pg.dsn("tidemark"),
        enabled: true,
        ..PostgresConfig::default()
    };
    let probe = probe_from_config(&cfg).unwrap();
    assert!(probe.reachable);
    assert!(probe.missing_privileges.is_empty());
    eprintln!("tested PostgreSQL version {:?}", probe.server_version_num);
    let mut collector = PgCollector::new(cfg);
    let samples = collector.collect_fast(100).unwrap();
    assert!(samples
        .iter()
        .any(|s| s.kind == "database" && s.name == "measured" && s.value > 0));
    assert!(samples.iter().any(|s| s.kind == "wal_dir" && s.value > 0));
    assert!(samples
        .iter()
        .any(|s| s.kind == "slot_retained" && s.name == "tidemark_test" && s.value >= 0));
    assert!(collector
        .collect_relations(100)
        .unwrap()
        .iter()
        .any(|s| s.name == "measured.public.measured" && s.value > 0));
    assert!(collector
        .path_labels()
        .unwrap()
        .iter()
        .any(|(_, k, v)| k == "pg.database" && v == "measured"));
}
#[test]
fn postgres_probe_reports_missing_monitor_privileges() {
    let pg = pgtmp::PgTmp::start();
    pg.admin()
        .batch_execute("CREATE ROLE limited LOGIN;")
        .unwrap();
    let probe = probe_from_config(&PostgresConfig {
        dsn: pg.dsn("limited"),
        ..PostgresConfig::default()
    })
    .unwrap();
    assert!(probe.reachable);
    assert!(probe
        .missing_privileges
        .iter()
        .any(|p| p == "pg_ls_waldir()"));
    assert!(probe
        .missing_privileges
        .iter()
        .any(|p| p == "data_directory"));
}
