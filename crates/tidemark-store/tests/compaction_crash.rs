//! Owned by: WCRASH-recovery
//! SIGKILL during compaction leaves an integral, idempotently compactable database.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use rusqlite::Connection;
use std::{
    hash::{Hash, Hasher},
    os::unix::process::ExitStatusExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use tidemark_core::config::SampleRetention;
use tidemark_store::timeseries::TimeSeries;
const NOW: i64 = 10_000_000;

#[test]
fn compaction_child() {
    let Some(path) = std::env::var_os("TIDEMARK_COMPACTION_CHILD") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let store = TimeSeries::open(&path, true).unwrap();
    std::fs::write(path.with_extension("ready"), b"ready").unwrap();
    store.compact(NOW, &SampleRetention::default()).unwrap();
}
fn digest(path: &Path) -> u64 {
    let conn = Connection::open(path).unwrap();
    let tables: Vec<String> = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap()
        .query_map([], |r| r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for table in tables {
        table.hash(&mut hash);
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM \"{table}\" LIMIT 0"))
            .unwrap();
        let count = stmt.column_count();
        drop(stmt);
        let order = (1..=count)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        stmt = conn
            .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY {order}"))
            .unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..count {
                format!("{:?}", row.get_ref(i).unwrap()).hash(&mut hash);
            }
        }
    }
    hash.finish()
}
#[test]
fn ten_compaction_crashes_recover_and_are_idempotent() {
    let dir = ::test_support::private_tempdir().unwrap();
    let seed = dir.path().join("seed.db");
    drop(TimeSeries::open(&seed, true).unwrap());
    let conn = Connection::open(&seed).unwrap();
    conn.execute_batch("WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<499999) INSERT INTO fs_samples (mount,res,ts,size,used,avail,inodes,inodes_used) SELECT '/data-'||(i/100),0,5000000+i%100,1000,i%1000,1000-i%1000,1000,i%1000 FROM n; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    drop(conn);
    let baseline = dir.path().join("baseline.db");
    std::fs::copy(&seed, &baseline).unwrap();
    let full = TimeSeries::open(&baseline, true).unwrap();
    let start = Instant::now();
    full.compact(NOW, &SampleRetention::default()).unwrap();
    let duration = start.elapsed();
    eprintln!("500000 sample compaction took {duration:?}");
    assert!(
        duration > Duration::from_millis(200),
        "seed must cover the longest kill delay"
    );
    drop(full);
    let expected = digest(&baseline);
    for delay in [0, 1, 2, 5, 10, 20, 40, 80, 120, 200] {
        let db = dir.path().join(format!("kill-{delay}.db"));
        std::fs::copy(&seed, &db).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "compaction_child", "--nocapture"])
            .env("TIDEMARK_COMPACTION_CHILD", &db)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !db.with_extension("ready").exists() {
            assert!(Instant::now() < deadline);
            assert!(child.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(delay));
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let store = TimeSeries::open(&db, true).unwrap();
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        drop(conn);
        store.compact(NOW, &SampleRetention::default()).unwrap();
        let first = digest(&db);
        assert_eq!(first, expected);
        assert_eq!(
            store.compact(NOW, &SampleRetention::default()).unwrap(),
            Default::default()
        );
        assert_eq!(digest(&db), first);
    }
}
