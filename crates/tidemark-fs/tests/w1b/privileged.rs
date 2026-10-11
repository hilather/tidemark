//! Owned by: W1-B
//! Disposable-VM-only atime, bind-mount and DT_UNKNOWN checks.
use super::*;
use std::process::Command;
use test_support::xfs::LoopXfs;
#[test]
fn noatime_no_writes() {
    let xfs = LoopXfs::new(512 * 1024 * 1024, &[], "strictatime");
    fs::create_dir(xfs.mountpoint.join("child")).unwrap();
    fs::write(xfs.mountpoint.join("child/file"), b"payload").unwrap();
    let before: Vec<_> = [&xfs.mountpoint, &xfs.mountpoint.join("child")]
        .into_iter()
        .map(|p| {
            let m = fs::metadata(p).unwrap();
            (m.atime(), m.atime_nsec())
        })
        .collect();
    let (s, _) = collect(&config(&xfs.mountpoint));
    assert_eq!(s.noatime_fallbacks, 0);
    let after: Vec<_> = [&xfs.mountpoint, &xfs.mountpoint.join("child")]
        .into_iter()
        .map(|p| {
            let m = fs::metadata(p).unwrap();
            (m.atime(), m.atime_nsec())
        })
        .collect();
    assert_eq!(before, after);
}
struct BindGuard(std::path::PathBuf);
impl Drop for BindGuard {
    fn drop(&mut self) {
        assert!(Command::new("umount")
            .arg(&self.0)
            .status()
            .unwrap()
            .success());
    }
}
#[test]
fn real_bind_mount_is_skipped() {
    let xfs = LoopXfs::new(512 * 1024 * 1024, &[], "noatime");
    let source = xfs.mountpoint.join("source");
    let target = xfs.mountpoint.join("bound");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("file"), []).unwrap();
    assert!(Command::new("mount")
        .arg("--bind")
        .arg(&source)
        .arg(&target)
        .status()
        .unwrap()
        .success());
    let _guard = BindGuard(target);
    let cfg = config(&xfs.mountpoint);
    let (s, rows) = collect(&cfg);
    assert_eq!(s.dirs, 2);
    let records = paths(rows, &cfg.root);
    assert!(records[&cfg.root]
        .skipped
        .iter()
        .any(|v| v.name.as_bytes() == b"bound" && v.reason == SkipReason::Mount));
}
#[test]
fn ftype_zero_walk_and_namespace() {
    let xfs = LoopXfs::new(
        512 * 1024 * 1024,
        &["-m", "crc=0", "-n", "ftype=0"],
        "noatime",
    );
    fs::create_dir(xfs.mountpoint.join("child")).unwrap();
    fs::write(xfs.mountpoint.join("child/file"), b"data").unwrap();
    std::os::unix::fs::symlink("child", xfs.mountpoint.join("link")).unwrap();
    let mut cfg = config(&xfs.mountpoint);
    let (s, rows) = collect(&cfg);
    assert_eq!((s.dirs, s.entries, s.errors), (2, 3, 0));
    exact_metadata(&cfg, rows);
    cfg.stat_entries = false;
    let (s, rows) = collect(&cfg);
    assert_eq!(s.dirs, 2);
    assert_eq!(
        rows.into_iter()
            .map(|r| match r {
                WalkOutput::Ns(r) => r.children.len(),
                _ => panic!(),
            })
            .sum::<usize>(),
        2
    );
}
