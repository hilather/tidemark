//! Owned by: WTEST-rules
//! Independent boundary, noatime and syscall-budget regressions.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tidemark_core::{
    path::RawPath,
    scan::{Gate, NoopGate, SkipReason, WalkOutput},
};
use tidemark_fs::walk::{self, WalkConfig};
#[derive(Default)]
struct Count(AtomicU64);
impl Gate for Count {
    fn acquire(&self, cost: u32) {
        assert_eq!(cost, 1);
        self.0.fetch_add(u64::from(cost), Ordering::Relaxed);
    }
    fn current_rate(&self) -> f64 {
        0.0
    }
}
fn opens(path: &Path) -> u64 {
    1 + path
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count() as u64
}
fn cfg(root: &Path) -> WalkConfig {
    WalkConfig {
        root: RawPath::from_path(root).unwrap(),
        threads: 1,
        stat_entries: true,
        exclude: globset::GlobSet::empty(),
        skip_mounts: vec![],
        big_file_threshold: u64::MAX,
        top_files: 0,
        scan_start: 2_000_000_000,
        getdents_buf: 4096,
        io_idle: false,
        max_record_bytes: 1024 * 1024,
    }
}
fn collect(c: &WalkConfig, g: &dyn Gate) -> (walk::WalkSummary, Vec<WalkOutput>) {
    let (tx, rx) = crossbeam_channel::bounded(8);
    std::thread::scope(|s| {
        let h = s.spawn(|| walk::walk(c, g, &AtomicBool::new(false), tx));
        let rows = rx.iter().collect();
        (h.join().unwrap().unwrap(), rows)
    })
}
#[test]
fn boundary_decisions_and_skip_reasons() {
    use walk::boundary_reason as decide;
    assert_eq!(decide(10, 20, 10, 20), None);
    assert_eq!(decide(10, 20, 11, 20), Some(SkipReason::OtherFs));
    assert_eq!(decide(10, 20, 10, 21), Some(SkipReason::OtherFs));
    assert_eq!(decide(10, 0, 10, 21), None);
    let t = tempfile::tempdir().unwrap();
    for name in ["mount", "excluded"] {
        fs::create_dir(t.path().join(name)).unwrap();
        fs::write(t.path().join(name).join("hidden"), b"hidden").unwrap();
    }
    let mut c = cfg(t.path());
    c.skip_mounts
        .push(RawPath::from_path(&t.path().join("mount")).unwrap());
    let mut b = globset::GlobSetBuilder::new();
    b.add(globset::Glob::new(&format!("{}/excluded", t.path().display())).unwrap());
    c.exclude = b.build().unwrap();
    let (summary, rows) = collect(&c, &NoopGate);
    assert_eq!(summary.dirs, 1);
    let WalkOutput::Dir(r) = &rows[0] else {
        panic!("expected root")
    };
    assert_eq!(r.own.files, 0);
    assert_eq!(r.skipped.len(), 2);
    assert_eq!(r.skipped[0].reason, SkipReason::Excluded);
    assert_eq!(r.skipped[1].reason, SkipReason::Mount);
}
#[test]
fn noatime_attempt_flags() {
    let gate = Count::default();
    let mut falls = 0;
    let mut flags = vec![];
    walk::open_attempts(&gate, &AtomicBool::new(false), &mut falls, |f| {
        flags.push(f);
        if flags.len() == 1 {
            Err(rustix::io::Errno::PERM)
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(
        flags,
        vec![rustix::fs::OFlags::NOATIME, rustix::fs::OFlags::empty()]
    );
    assert_eq!(falls, 1);
    assert_eq!(gate.0.load(Ordering::Relaxed), 2);
}
#[test]
fn owned_directories_have_no_fallbacks() {
    let t = tempfile::tempdir().unwrap();
    let mut falls = 0;
    walk::open_directory(
        &RawPath::from_path(t.path()).unwrap(),
        &NoopGate,
        &mut falls,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(falls, 0);
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    fs::File::open(t.path())
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_accessed(old)
                .set_modified(old + std::time::Duration::from_secs(1000)),
        )
        .unwrap();
    let before = fs::metadata(t.path()).unwrap();
    assert!(before.atime() < before.mtime());
    let (summary, _) = collect(&cfg(t.path()), &NoopGate);
    assert_eq!(summary.noatime_fallbacks, 0);
    let after = fs::metadata(t.path()).unwrap();
    assert_eq!(
        (after.atime(), after.atime_nsec()),
        (before.atime(), before.atime_nsec())
    );
}
#[test]
fn foreign_directory_fallback_count() {
    let uid = rustix::process::geteuid().as_raw();
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let caps = status
        .lines()
        .find_map(|l| l.strip_prefix("CapEff:"))
        .unwrap();
    let fowner = u64::from_str_radix(caps.trim(), 16).unwrap() & (1 << 3) != 0;
    let foreign_fixture = tempfile::tempdir().unwrap();
    let candidate = ["/usr", "/", "/opt", "/tmp"]
        .into_iter()
        .map(Path::new)
        .find(|p| {
            fs::symlink_metadata(p)
                .is_ok_and(|m| m.is_dir() && m.uid() != uid && m.mode() & 0o555 == 0o555)
        });
    let path = candidate.unwrap_or_else(|| {
        // Real-root CI may own every conventional system directory. Only in
        // that case create a foreign owner inside our own temporary fixture.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(foreign_fixture.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let other_uid = if uid == 0 { 65534 } else { 0 };
        rustix::fs::chown(
            foreign_fixture.path(),
            Some(rustix::process::Uid::from_raw(other_uid)),
            None,
        )
        .expect("need a foreign-owned directory; cannot silently skip");
        foreign_fixture.path()
    });
    let mut falls = 0;
    walk::open_directory(
        &RawPath::from_path(path).unwrap(),
        &NoopGate,
        &mut falls,
        &AtomicBool::new(false),
    )
    .unwrap();
    // Ancestors use O_PATH, which ignores NOATIME; only the final O_RDONLY open
    // can require the EPERM retry. Exactly one foreign directory is opened for reading.
    let foreign = fs::metadata(path).unwrap().uid() != uid;
    assert_eq!(falls, u64::from(foreign && !fowner));
}
#[test]
fn exact_walk_syscall_budget_from_shape() {
    let t = tempfile::tempdir().unwrap();
    let child = t.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(t.path().join("a"), b"a").unwrap();
    fs::write(child.join("b"), b"b").unwrap();
    let gate = Count::default();
    let (s, rows) = collect(&cfg(t.path()), &gate);
    assert_eq!((s.dirs, s.errors, s.races), (2, 0, 0));
    assert_eq!(rows.len(), 2);
    // Root preflight: component opens, identity stat, mountinfo. Each scanned dir:
    // component opens, identity stat, one small getdents batch plus EOF. Two file stats.
    let expected = opens(t.path()) + 2 + opens(t.path()) + 3 + opens(&child) + 3 + 2;
    assert_eq!(gate.0.load(Ordering::Relaxed), expected);
}
#[test]
fn symlink_target_read_is_charged() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("file"), b"x").unwrap();
    let root = RawPath::from_path(t.path()).unwrap();
    let cancel = AtomicBool::new(false);
    let opts = tidemark_fs::symlinks::SymlinkOpts {
        check_targets: false,
        limit: 10,
    };
    let baseline = Count::default();
    tidemark_fs::symlinks::analyze(&root, &baseline, &cancel, &opts).unwrap();
    std::os::unix::fs::symlink("file", t.path().join("link")).unwrap();
    let with_link = Count::default();
    let r = tidemark_fs::symlinks::analyze(&root, &with_link, &cancel, &opts).unwrap();
    assert_eq!(r.total, 1);
    // One extra entry stat and one readlink; all directory operations are identical.
    assert_eq!(
        with_link.0.load(Ordering::Relaxed),
        baseline.0.load(Ordering::Relaxed) + 2
    );
}
