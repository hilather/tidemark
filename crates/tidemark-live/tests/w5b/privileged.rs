//! Owned by: W5-B
//! Child-process fixtures for deleted files and writable growth; pending VM.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::fs::MetadataExt,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tidemark_core::path::RawPath;
use tidemark_live::procscan::{ProcScanner, ProcScannerOptions};
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(script: &str, path: &std::path::Path) -> Guard {
    let c = Command::new("python3")
        .args(["-u", "-c", script])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    Guard(c)
}
fn ready(c: &mut Guard) {
    let mut line = String::new();
    BufReader::new(c.0.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready");
}
fn scanner(dir: &std::path::Path) -> ProcScanner {
    ProcScanner::new(
        ProcScannerOptions {
            mounts: vec![(
                RawPath::from_path(dir).unwrap(),
                std::fs::metadata(dir).unwrap().dev(),
            )],
            open_writers: true,
            exclude_prefixes: vec![],
        },
        Arc::new(tidemark_fs::attribution::AttributionCache::new(
            Duration::from_secs(60),
            10000,
        )),
    )
}
#[test]
fn phantom_detects_deleted_open() {
    assert!(rustix::process::geteuid().is_root());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phantom");
    let mut c = child("import os,sys\nf=open(sys.argv[1],'wb')\nf.write(b'x'*(100*1024*1024)); f.flush(); os.unlink(sys.argv[1]); os.setuid(65534); print('ready'); sys.stdin.readline()",&path);
    ready(&mut c);
    let expected = std::fs::metadata(format!("/proc/{}/fd/3", c.0.id())).unwrap();
    let comm = tidemark_fs::procfs::read_comm(c.0.id()).unwrap();
    let mut s = scanner(dir.path());
    s.scan_once(0).unwrap();
    let rows = s.phantom(None);
    let h = rows[0]
        .holders
        .iter()
        .find(|h| h.pid == c.0.id() && h.ino == expected.ino())
        .unwrap();
    assert_eq!(h.bytes, expected.blocks() * 512);
    assert_eq!(h.comm, comm);
    assert_eq!(h.path, path.as_os_str().as_encoded_bytes());
}
#[test]
fn open_writer_growth() {
    assert!(rustix::process::geteuid().is_root());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("writer");
    let mut c = child("import sys\nf=open(sys.argv[1],'wb'); f.write(b'x'*1048576); f.flush(); print('ready'); sys.stdin.readline(); f.write(b'x'*1048576); f.flush(); print('ready'); sys.stdin.readline()",&path);
    ready(&mut c);
    let readonly = std::fs::File::open(&path).unwrap();
    let before = readonly.metadata().unwrap();
    let mut s = scanner(dir.path());
    s.scan_once(0).unwrap();
    writeln!(c.0.stdin.as_mut().unwrap(), "grow").unwrap();
    ready(&mut c);
    let after = readonly.metadata().unwrap();
    s.scan_once(10).unwrap();
    let rows = s.writers(None, Duration::from_secs(10), 100);
    let row = rows.iter().find(|r| r.ino == after.ino()).unwrap();
    assert_eq!(row.pid, c.0.id());
    assert_eq!(
        row.growth,
        (after.blocks() as i64 - before.blocks() as i64) * 512
    );
}
