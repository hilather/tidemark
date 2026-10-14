//! Owned by: WSTORE-locks
//! Separate-process evidence that repeated preflight preserves SQLite locks.
use std::process::Command;
use tidemark_store::timeseries::TimeSeries;

#[test]
fn repeated_open_preserves_main_file_locks() {
    let state = tempfile::tempdir().unwrap();
    let path = state.path().join("timeseries.db");
    let first = TimeSeries::open(&path, true).unwrap();
    first.query_fs("/", 0, 1).unwrap();
    let probe = || {
        Command::new("python3")
            .args(["-c", r#"
import errno, fcntl, sys
with open(sys.argv[1], 'r+b') as file:
    try:
        fcntl.lockf(file, fcntl.LOCK_EX | fcntl.LOCK_NB, 510, 0x40000002)
    except OSError as error:
        if error.errno in (errno.EACCES, errno.EAGAIN):
            sys.exit(0)
        raise
sys.exit(1)
"#])
            .arg(&path)
            .status().unwrap().success()
    };
    assert!(probe(), "first connection must hold the shared range");
    let second = TimeSeries::open(&path, false).unwrap();
    second.query_fs("/", 0, 1).unwrap();
    assert!(probe(), "second preflight must preserve the shared range");
    drop(second);
    assert!(probe(), "closing one connection must preserve the other's locks");
    drop(first);
    assert!(!probe(), "probe must acquire the range after all connections close");
}
