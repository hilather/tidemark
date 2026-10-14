//! Owned by: TSW-compact
//! Repeated compaction witnesses through independent SQLite connections.
use super::*;
use std::sync::atomic::AtomicI64;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

fn sql(path: &std::path::Path, script: &str, mount: &str, now: i64) {
    let output = std::process::Command::new("python3")
        .args(["-c", script])
        .arg(path)
        .arg(mount)
        .arg(now.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
const INSERT: &str = r#"
import sqlite3,sys
with sqlite3.connect(sys.argv[1],timeout=5) as db:
 base=(int(sys.argv[3])-3*86400)//300*300
 db.executemany('INSERT INTO fs_samples (mount,res,ts,size,used,avail,inodes,inodes_used) VALUES (?,0,?,2000000,?,700000,2000,200)', ((sys.argv[2],base+n,n) for n in range(600)))
"#;
const RAW: &str = r#"
import sqlite3,sys
with sqlite3.connect(sys.argv[1],timeout=5) as db:
 assert db.execute('SELECT res,count(*) FROM fs_samples WHERE mount=? GROUP BY res',(sys.argv[2],)).fetchall()==[(0,600)]
"#;
const CHECK: &str = r#"
import sqlite3,sys
with sqlite3.connect(sys.argv[1],timeout=5) as db:
 base=(int(sys.argv[3])-3*86400)//300*300
 actual=db.execute('SELECT res,ts,size,used,avail,inodes,inodes_used FROM fs_samples WHERE mount=? ORDER BY res,ts',(sys.argv[2],)).fetchall()
 expected=[(1,base+n*300,2000000,n*300+299,700000,2000,200) for n in range(2)]
 assert actual==expected, (len(actual),actual[:2])
"#;
fn barrier(tx: &Sender) {
    let (ack, rx) = crossbeam_channel::bounded(1);
    tx.send(Message::Flush(ack)).unwrap();
    rx.recv_timeout(Duration::from_secs(15)).unwrap();
    // Flush acknowledges before maintenance; another message orders after it.
    let (ack, rx) = crossbeam_channel::bounded(1);
    tx.send(Message::Flush(ack)).unwrap();
    rx.recv_timeout(Duration::from_secs(15)).unwrap();
}
fn repeated(rounds: i64, duration: i64) {
    let dir = tempfile::tempdir().unwrap();
    let mut config = tidemark_core::config::Config::default();
    config.roots.clear();
    config.general.state_dir = dir.path().into();
    let state = crate::state::LiveState::new(&config, 0);
    let stop = crate::Shutdown::default();
    let elapsed = Arc::new(AtomicI64::new(0));
    let wall = Arc::new(AtomicI64::new(10 * 86400));
    let clock: crate::Clock = {
        let wall = wall.clone();
        Arc::new(move || wall.load(Ordering::Acquire))
    };
    let mono: crate::Clock = {
        let elapsed = elapsed.clone();
        Arc::new(move || {
            let now = elapsed.load(Ordering::Acquire);
            if now == 3600 {
                elapsed.fetch_add(duration, Ordering::AcqRel);
            }
            now
        })
    };
    let (tx, handle) =
        start_with_clocks(Arc::new(config), state, stop.clone(), clock, mono).unwrap();
    barrier(&tx);
    let path = dir.path().join("timeseries.db");
    for round in 1..=rounds {
        let mount = format!("__tidemark_fault_compaction__{round}_second__");
        let now = 10 * 86400 + round * 3600;
        wall.store(now, Ordering::Release);
        sql(&path, INSERT, &mount, now);
        // Wall time changes alone cannot trigger operational maintenance.
        elapsed.store(round * 3600 - 1, Ordering::Release);
        barrier(&tx);
        sql(&path, RAW, &mount, now);
        elapsed.store(round * 3600, Ordering::Release);
        barrier(&tx);
        sql(&path, CHECK, &mount, now);
    }
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}
#[test]
fn two_compactions_see_external_witnesses() {
    repeated(2, 0);
}
#[test]
fn three_compactions_do_not_drift_by_duration() {
    repeated(3, 44);
}

#[test]
fn restart_preserves_deadline_and_logs_compaction_results() {
    const CHILD: &str = "TIDEMARK_COMPACT_LOG_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "services::tswriter::compact_tests::restart_preserves_deadline_and_logs_compaction_results", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    #[derive(Clone)]
    struct Sink(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let logs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Sink(logs.clone());
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut config = tidemark_core::config::Config::default();
    config.roots.clear();
    config.general.state_dir = dir.path().into();
    let state = crate::state::LiveState::new(&config, 0);
    let stop = crate::Shutdown::default();
    let elapsed = Arc::new(AtomicI64::new(0));
    let wall = Arc::new(AtomicI64::new(10 * 86400));
    let (backoff, waiting) = crossbeam_channel::bounded(1);
    let mono: crate::Clock = {
        let elapsed = elapsed.clone();
        Arc::new(move || {
            let now = elapsed.load(Ordering::Acquire);
            if now == 3000 && std::thread::current().name() == Some("ts-supervisor") {
                let _ = backoff.try_send(());
            }
            now
        })
    };
    let clock: crate::Clock = {
        let wall = wall.clone();
        Arc::new(move || wall.load(Ordering::Acquire))
    };
    let (tx, handle) =
        start_supervised_with_clocks(Arc::new(config), state.clone(), stop.clone(), clock, mono)
            .unwrap();
    barrier(&tx);
    elapsed.store(3000, Ordering::Release);
    tx.send(Message::Panic).unwrap();
    // The clock seam acknowledges the backoff's initial read, with no sleeps
    // deciding whether the worker has restarted.
    waiting.recv_timeout(Duration::from_secs(15)).unwrap();
    elapsed.store(3005, Ordering::Release);
    barrier(&tx);
    let path = dir.path().join("timeseries.db");
    let mount = "__tidemark_fault_compaction___second__";
    let now = wall.load(Ordering::Acquire);
    sql(&path, INSERT, mount, now);
    // Restarted writer must retain its original deadline (at most just overdue).
    elapsed.store(3599, Ordering::Release);
    barrier(&tx);
    sql(&path, RAW, mount, now);
    elapsed.store(3600, Ordering::Release);
    barrier(&tx);
    sql(&path, CHECK, mount, now);
    // A cutoff overflow fails and logs, without resetting the worker/schedule.
    wall.store(i64::MIN, Ordering::Release);
    elapsed.store(7200, Ordering::Release);
    barrier(&tx);
    wall.store(now, Ordering::Release);
    sql(&path, INSERT, "__tidemark_fault_compaction___third__", now);
    elapsed.store(10800, Ordering::Release);
    barrier(&tx);
    sql(&path, CHECK, "__tidemark_fault_compaction___third__", now);
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    let text = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    let warnings: Vec<_> = text
        .lines()
        .filter(|l| l.contains("WARN") && l.contains("restarting time-series writer"))
        .collect();
    assert_eq!(warnings.len(), 1, "{text}");
    assert!(warnings[0].contains("injected writer death"));
    let info: Vec<_> = text
        .lines()
        .filter(|l| l.contains("INFO") && l.contains("time-series compaction completed"))
        .collect();
    assert_eq!(info.len(), 2, "{text}");
    for line in info {
        assert!(
            line.contains("duration_ms=")
                && line.contains("inserted=2")
                && line.contains("deleted=600"),
            "{text}"
        );
    }
    assert_eq!(
        text.lines()
            .filter(|l| l.contains("ERROR") && l.contains("time-series writer failure"))
            .count(),
        1,
        "{text}"
    );
}
