//! Owned by: TSW-compact
//! Repeated compaction witnesses through independent SQLite connections.
use super::*;
use std::{sync::{atomic::Ordering, Arc}, time::Duration};
use std::sync::atomic::AtomicI64;

fn sql(path: &std::path::Path, script: &str, mount: &str, now: i64) {
    let output = std::process::Command::new("python3")
        .args(["-c", script])
        .arg(path)
        .arg(mount)
        .arg(now.to_string())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
const INSERT: &str = r#"
import sqlite3,sys
with sqlite3.connect(sys.argv[1],timeout=5) as db:
 base=(int(sys.argv[3])-3*86400)//300*300
 db.executemany('INSERT INTO fs_samples (mount,res,ts,size,used,avail,inodes,inodes_used) VALUES (?,0,?,2000000,?,700000,2000,200)', ((sys.argv[2],base+n,n) for n in range(600)))
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
    let (tx, handle) = start_with_clocks(Arc::new(config), state, stop.clone(), clock, mono).unwrap();
    barrier(&tx);
    let path = dir.path().join("timeseries.db");
    for round in 1..=rounds {
        let mount = format!("__tidemark_fault_compaction__{round}_second__");
        let now = 10 * 86400 + round * 3600;
        wall.store(now, Ordering::Release);
        sql(&path, INSERT, &mount, now);
        elapsed.store(round * 3600, Ordering::Release);
        barrier(&tx);
        sql(&path, CHECK, &mount, now);
    }
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}
#[test]
fn two_compactions_see_external_witnesses() { repeated(2, 0); }
#[test]
fn three_compactions_do_not_drift_by_duration() { repeated(3, 44); }
