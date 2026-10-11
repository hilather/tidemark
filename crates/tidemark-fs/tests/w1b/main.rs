//! Owned by: W1-B
//! Independent metadata checks for both walker modes and resource safety.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tidemark_core::{path::RawPath, scan::*};
use tidemark_fs::walk::{read_dir_entries, walk, WalkConfig, WalkError, WalkSummary};

fn config(root: &std::path::Path) -> WalkConfig {
    WalkConfig {
        root: RawPath::from_path(root).unwrap(),
        threads: 4,
        stat_entries: true,
        exclude: globset::GlobSet::empty(),
        skip_mounts: Vec::new(),
        big_file_threshold: 0,
        top_files: 10,
        scan_start: 2_000_000_000,
        getdents_buf: 4096,
        io_idle: true,
    }
}
fn collect(cfg: &WalkConfig) -> (WalkSummary, Vec<WalkOutput>) {
    let (tx, rx) = crossbeam_channel::bounded(4);
    std::thread::scope(|scope| {
        let producer = scope.spawn(|| walk(cfg, &NoopGate, &AtomicBool::new(false), tx));
        let rows = rx.iter().collect();
        (producer.join().unwrap().unwrap(), rows)
    })
}
fn fixture(name: &str) -> test_support::Fixture {
    test_support::materialize_file(format!(
        "{}/../test-support/fixtures/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
}
fn paths(rows: Vec<WalkOutput>, root: &RawPath) -> BTreeMap<RawPath, DirRecord> {
    let mut pending: Vec<_> = rows
        .into_iter()
        .map(|r| match r {
            WalkOutput::Dir(r) => r,
            _ => panic!("unexpected output"),
        })
        .collect();
    let mut ids = BTreeMap::new();
    let mut result = BTreeMap::new();
    while !pending.is_empty() {
        let n = pending.len();
        pending.retain(|r| {
            let path = match r.parent {
                None => root.clone(),
                Some(parent) => match ids.get(&parent) {
                    Some(p) => RawPath::join(p, r.name.as_ref().unwrap()),
                    None => return true,
                },
            };
            ids.insert(r.id, path.clone());
            result.insert(path, r.clone());
            false
        });
        assert!(pending.len() < n);
    }
    result
}
fn exact_metadata(cfg: &WalkConfig, rows: Vec<WalkOutput>) {
    let records = paths(rows, &cfg.root);
    let mut expected_dirs = std::collections::BTreeSet::new();
    let mut pending = vec![cfg.root.clone()];
    while let Some(path) = pending.pop() {
        expected_dirs.insert(path.clone());
        for entry in fs::read_dir(path.as_path()).unwrap() {
            let entry = entry.unwrap();
            if fs::symlink_metadata(entry.path()).unwrap().is_dir() {
                pending.push(RawPath::from_path(&entry.path()).unwrap());
            }
        }
    }
    assert_eq!(
        records
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected_dirs
    );
    for (path, r) in records {
        let meta = fs::symlink_metadata(path.as_path()).unwrap();
        assert_eq!(
            (r.ino, r.dev, r.mtime, r.dir_disk, r.dir_apparent),
            (
                meta.ino(),
                meta.dev(),
                meta.mtime(),
                meta.blocks() * 512,
                meta.size()
            )
        );
        let mut own = OwnStats::default();
        let mut links = Vec::new();
        let mut notable = Vec::new();
        let mut count = 0;
        for e in fs::read_dir(path.as_path()).unwrap() {
            let e = e.unwrap();
            count += 1;
            let m = fs::symlink_metadata(e.path()).unwrap();
            if m.is_dir() {
                continue;
            }
            let name =
                tidemark_core::path::RawName::from_path(std::path::Path::new(&e.file_name()))
                    .unwrap();
            let kind = if m.is_file() {
                FileKind::Regular
            } else if m.file_type().is_symlink() {
                FileKind::Symlink
            } else {
                FileKind::Other
            };
            if m.nlink() > 1 {
                links.push(LinkRef {
                    ino: m.ino(),
                    nlink: m.nlink() as u32,
                    kind,
                    disk: m.blocks() * 512,
                    apparent: m.size(),
                    mtime: m.mtime(),
                    name: Some(name),
                });
            } else {
                own.files += 1;
                own.symlinks += u64::from(kind == FileKind::Symlink);
                own.disk += m.blocks() * 512;
                own.apparent += m.size();
                own.age.0[tidemark_core::time::age_bucket(cfg.scan_start, m.mtime())] +=
                    m.blocks() * 512;
                if own.files == 1 || m.mtime() > own.newest_mtime {
                    own.newest_mtime = m.mtime();
                }
                notable.push(NotableFile {
                    ino: m.ino(),
                    name: Some(name),
                    disk: m.blocks() * 512,
                    apparent: m.size(),
                    mtime: m.mtime(),
                });
            }
        }
        links.sort_by(|a, b| a.name.cmp(&b.name));
        notable.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(r.entries, count);
        assert_eq!(r.own, own);
        assert_eq!(r.links, links);
        assert_eq!(r.notable, notable);
        assert!(r.errors.is_empty());
        assert!(r.skipped.is_empty());
        assert_eq!(r.races, 0);
        assert_eq!(r.projid, None);
    }
}
#[test]
fn link_semantics_basic_exact_records() {
    let f = fixture("link_semantics_basic");
    let cfg = config(&f.root);
    let (s, rows) = collect(&cfg);
    assert_eq!((s.dirs, s.entries, s.errors, s.races), (6, 12, 0, 0));
    exact_metadata(&cfg, rows);
}
#[test]
fn metadata_fixtures() {
    for name in ["symlink_loops", "nonutf8", "hardlink_web", "sparse", "deep"] {
        let f = fixture(name);
        let cfg = config(&f.root);
        let (s, rows) = collect(&cfg);
        assert_eq!(s.errors, 0, "{name}");
        exact_metadata(&cfg, rows);
        if name == "deep" {
            assert_eq!(s.dirs, 201);
        }
    }
}
#[test]
fn namespace_and_live_names() {
    let f = fixture("nonutf8");
    let mut cfg = config(&f.root);
    cfg.stat_entries = false;
    let (s, rows) = collect(&cfg);
    assert_eq!(s.errors, 0);
    let mut records: Vec<_> = rows
        .into_iter()
        .map(|r| match r {
            WalkOutput::Ns(r) => r,
            _ => panic!("unexpected record"),
        })
        .collect();
    let mut dirs = BTreeMap::<DirId, RawPath>::new();
    while !records.is_empty() {
        let n = records.len();
        records.retain(|r| {
            let path = match r.parent {
                None => cfg.root.clone(),
                Some(p) => match dirs.get(&p) {
                    Some(path) => path.join(r.name.as_ref().unwrap()),
                    None => return true,
                },
            };
            let entries = read_dir_entries(&path, &NoopGate).unwrap();
            let mut names: Vec<_> = fs::read_dir(path.as_path())
                .unwrap()
                .map(|e| {
                    tidemark_core::path::RawName::from_path(std::path::Path::new(
                        &e.unwrap().file_name(),
                    ))
                    .unwrap()
                })
                .collect();
            names.sort();
            assert_eq!(
                entries.iter().map(|e| e.0.clone()).collect::<Vec<_>>(),
                names
            );
            let mut expected: Vec<_> = entries
                .iter()
                .filter(|e| e.2 != FileKind::Dir)
                .map(|e| e.1)
                .collect();
            let mut found = r.children.clone();
            expected.sort();
            found.sort();
            assert_eq!(found, expected);
            for (name, ino, _) in entries {
                assert_eq!(
                    fs::symlink_metadata(path.join(&name).as_path())
                        .unwrap()
                        .ino(),
                    ino
                );
            }
            dirs.insert(r.id, path);
            false
        });
        assert!(records.len() < n);
    }
}
#[test]
fn exclusions_and_mounts() {
    let f = fixture("link_semantics_basic");
    let mut cfg = config(&f.root);
    cfg.skip_mounts
        .push(RawPath::from_path(&f.root.join("a")).unwrap());
    let mut globs = globset::GlobSetBuilder::new();
    globs.add(globset::Glob::new(&format!("{}/c", f.root.display())).unwrap());
    globs.add(globset::Glob::new(&format!("{}/b/z", f.root.display())).unwrap());
    cfg.exclude = globs.build().unwrap();
    let (s, rows) = collect(&cfg);
    assert_eq!((s.dirs, s.entries), (2, 5));
    let records = paths(rows, &cfg.root);
    let root = &records[&cfg.root];
    assert_eq!(
        root.skipped
            .iter()
            .map(|x| x.reason.clone())
            .collect::<Vec<_>>(),
        vec![SkipReason::Mount, SkipReason::Excluded]
    );
    let b = &records[&RawPath::from_path(&f.root.join("b")).unwrap()];
    assert_eq!(b.skipped[0].reason, SkipReason::Excluded);
}
#[test]
fn cancellation_backpressure_and_disconnect() {
    let f = fixture("deep");
    let cfg = config(&f.root);
    let cancel = AtomicBool::new(false);
    let (tx, _rx) = crossbeam_channel::bounded(1);
    std::thread::scope(|scope| {
        let h = scope.spawn(|| walk(&cfg, &NoopGate, &cancel, tx));
        std::thread::sleep(Duration::from_millis(30));
        let start = Instant::now();
        cancel.store(true, Ordering::Release);
        assert!(matches!(h.join().unwrap(), Err(WalkError::Cancelled)));
        assert!(start.elapsed() < Duration::from_secs(1));
    });
    let (tx, rx) = crossbeam_channel::bounded(1);
    drop(rx);
    assert!(matches!(
        walk(&cfg, &NoopGate, &AtomicBool::new(false), tx),
        Err(WalkError::Disconnected)
    ));
}
struct CountingGate(AtomicUsize);
impl Gate for CountingGate {
    fn acquire(&self, c: u32) {
        self.0.fetch_add(c as usize, Ordering::Relaxed);
    }
    fn current_rate(&self) -> f64 {
        f64::INFINITY
    }
}
#[test]
fn namespace_avoids_file_stats() {
    let temp = tempfile::tempdir().unwrap();
    for i in 0..100 {
        fs::write(temp.path().join(i.to_string()), []).unwrap();
    }
    let mut cfg = config(temp.path());
    cfg.stat_entries = false;
    let gate = CountingGate(AtomicUsize::new(0));
    let (tx, rx) = crossbeam_channel::bounded(1);
    walk(&cfg, &gate, &AtomicBool::new(false), tx).unwrap();
    assert_eq!(rx.iter().count(), 1);
    let namespace = gate.0.swap(0, Ordering::Relaxed);
    cfg.stat_entries = true;
    let (tx, rx) = crossbeam_channel::bounded(1);
    walk(&cfg, &gate, &AtomicBool::new(false), tx).unwrap();
    assert_eq!(rx.iter().count(), 1);
    assert_eq!(gate.0.load(Ordering::Relaxed) - namespace, 100);
}
#[test]
fn symlink_ancestor_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("real")).unwrap();
    std::os::unix::fs::symlink("real", temp.path().join("alias")).unwrap();
    let cfg = config(&temp.path().join("alias"));
    let (tx, _rx) = crossbeam_channel::bounded(1);
    assert!(walk(&cfg, &NoopGate, &AtomicBool::new(false), tx).is_err());
    assert!(read_dir_entries(&cfg.root, &NoopGate).is_err());
}
#[cfg(feature = "privileged")]
mod privileged;
#[test]
fn random_tree_records() {
    for seed in 0..4 {
        let f = test_support::random_tree(seed, Default::default());
        let cfg = config(&f.root);
        exact_metadata(&cfg, collect(&cfg).1);
    }
}
#[test]
#[ignore = "scale"]
fn wide_dir_memory_and_fd_bound() {
    // Sample in an isolated test process so unrelated concurrently running tests
    // cannot inflate the descriptor count or RSS baseline.
    if std::env::var_os("W1B_RESOURCE_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "wide_dir_memory_and_fd_bound",
                "--ignored",
                "--nocapture",
            ])
            .env("W1B_RESOURCE_CHILD", "1")
            .output()
            .unwrap();
        eprintln!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success());
        return;
    }
    let f = fixture("wide_dir");
    let mut cfg = config(&f.root);
    cfg.big_file_threshold = u64::MAX;
    cfg.top_files = 0;
    fn rss() -> u64 {
        fs::read_to_string("/proc/self/statm")
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u64>()
            .unwrap()
            * 4096
    }
    fn fds() -> usize {
        fs::read_dir("/proc/self/fd").unwrap().count()
    }
    let baseline = rss();
    let baseline_fd = fds();
    let done = AtomicBool::new(false);
    let peak = AtomicUsize::new(0);
    let peak_fd = AtomicUsize::new(0);
    let (s, rows) = std::thread::scope(|scope| {
        let sampler = scope.spawn(|| {
            while !done.load(Ordering::Acquire) {
                peak.fetch_max(rss() as usize, Ordering::Relaxed);
                peak_fd.fetch_max(fds(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let output = collect(&cfg);
        done.store(true, Ordering::Release);
        sampler.join().unwrap();
        output
    });
    eprintln!(
        "wide_dir: RSS delta {} bytes; peak fds {} (baseline {}), limit {}",
        (peak.load(Ordering::Relaxed) as u64).saturating_sub(baseline),
        peak_fd.load(Ordering::Relaxed),
        baseline_fd,
        cfg.threads + 8
    );
    assert_eq!((s.entries, s.errors), (120001, 0));
    assert!(rows
        .iter()
        .any(|r| matches!(r,WalkOutput::Dir(r) if r.entries==120000)));
    assert!((peak.load(Ordering::Relaxed) as u64).saturating_sub(baseline) < 256 * 1024 * 1024);
    assert!(peak_fd.load(Ordering::Relaxed) <= cfg.threads + 8);
}
#[test]
fn paths_beyond_path_max() {
    use rustix::fs::{mkdirat, openat, Mode, OFlags, CWD};
    let temp = tempfile::tempdir().unwrap();
    let mut fd = openat(
        CWD,
        temp.path(),
        OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .unwrap();
    let component = "abcdefghijklmnopqrstuvwx";
    for _ in 0..200 {
        mkdirat(&fd, component, Mode::RWXU).unwrap();
        fd = openat(
            &fd,
            component,
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .unwrap();
    }
    drop(fd);
    let (s, _) = collect(&config(temp.path()));
    assert_eq!((s.dirs, s.errors), (201, 0));
    // tempfile's recursive cleanup is descriptor-relative on supported platforms.
}
#[test]
fn deleted_queued_child_is_a_race() {
    struct DeleteGate {
        calls: AtomicUsize,
        trigger: usize,
        target: std::path::PathBuf,
    }
    impl Gate for DeleteGate {
        fn acquire(&self, _: u32) {
            if self.calls.fetch_add(1, Ordering::Relaxed) == self.trigger {
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| fs::remove_dir_all(&self.target).unwrap())
                        .join()
                        .unwrap();
                });
            }
        }
        fn current_rate(&self) -> f64 {
            f64::INFINITY
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("child");
    fs::create_dir(&child).unwrap();
    let mut cfg = config(temp.path());
    cfg.threads = 1;
    // Calibrate the final root getdents call including O_NOATIME retries.
    let counter = CountingGate(AtomicUsize::new(0));
    let mut exclusions = globset::GlobSetBuilder::new();
    exclusions.add(globset::Glob::new(&format!("{}/child", temp.path().display())).unwrap());
    cfg.exclude = exclusions.build().unwrap();
    let (tx, rx) = crossbeam_channel::bounded(4);
    walk(&cfg, &counter, &AtomicBool::new(false), tx).unwrap();
    assert_eq!(rx.iter().count(), 1);
    cfg.exclude = globset::GlobSet::empty();
    let gate = DeleteGate {
        calls: AtomicUsize::new(0),
        trigger: counter.0.load(Ordering::Relaxed) - 1,
        target: child,
    };
    let (tx, rx) = crossbeam_channel::bounded(4);
    let s = walk(&cfg, &gate, &AtomicBool::new(false), tx).unwrap();
    assert_eq!((s.errors, s.races), (0, 1));
    assert!(rx
        .iter()
        .any(|r| matches!(r,WalkOutput::Gone(g) if g.reason==GoneReason::Vanished)));
}

#[test]
fn mixed_special_records_when_supported() {
    let spec = fs::read_to_string(format!(
        "{}/../test-support/fixtures/mixed_special.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    match test_support::fixture::try_materialize(&spec) {
        Ok(f) => {
            let cfg = config(&f.root);
            exact_metadata(&cfg, collect(&cfg).1);
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("mixed_special socket fixture unavailable under sandbox policy: {e}");
            let f = test_support::materialize("[[entry]]\npath='pipe'\nkind='fifo'");
            let cfg = config(&f.root);
            exact_metadata(&cfg, collect(&cfg).1);
        }
        Err(e) => panic!("fixture failed: {e}"),
    }
}
#[test]
fn notable_threshold_and_worker_top_files() {
    let temp = tempfile::tempdir().unwrap();
    for (name, len) in [("small", 4096), ("medium", 8192), ("large", 16384)] {
        fs::write(temp.path().join(name), vec![1; len]).unwrap();
    }
    let mut cfg = config(temp.path());
    cfg.threads = 1;
    cfg.big_file_threshold = u64::MAX;
    cfg.top_files = 2;
    let (_, rows) = collect(&cfg);
    let records = paths(rows, &cfg.root);
    let notable = &records[&cfg.root].notable;
    assert!(notable
        .iter()
        .any(|f| f.name.as_ref().unwrap().as_bytes() == b"large"));
    assert!(notable
        .iter()
        .any(|f| f.name.as_ref().unwrap().as_bytes() == b"medium"));
    cfg.top_files = 0;
    cfg.big_file_threshold = fs::metadata(temp.path().join("large")).unwrap().blocks() * 512;
    let (_, rows) = collect(&cfg);
    let records = paths(rows, &cfg.root);
    let notable = &records[&cfg.root].notable;
    assert_eq!(notable.len(), 1);
    assert_eq!(notable[0].name.as_ref().unwrap().as_bytes(), b"large");
}
#[test]
fn settings_and_channel_limits_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = config(temp.path());
    let cancel = AtomicBool::new(false);
    for (threads, buffer) in [(0, 4096), (65, 4096), (1, 0), (1, 5 * 1024 * 1024)] {
        cfg.threads = threads;
        cfg.getdents_buf = buffer;
        let (tx, _rx) = crossbeam_channel::bounded(1);
        assert!(matches!(
            walk(&cfg, &NoopGate, &cancel, tx),
            Err(WalkError::Limit(_))
        ));
    }
    cfg = config(temp.path());
    let (tx, _rx) = crossbeam_channel::unbounded();
    assert!(matches!(
        walk(&cfg, &NoopGate, &cancel, tx),
        Err(WalkError::Limit(_))
    ));
    let (tx, _rx) = crossbeam_channel::bounded(1);
    cancel.store(true, Ordering::Release);
    assert!(matches!(
        walk(&cfg, &NoopGate, &cancel, tx),
        Err(WalkError::Cancelled)
    ));
}
#[test]
fn worker_panic_joins_instead_of_hanging() {
    struct PanicGate {
        calls: AtomicUsize,
        trigger: usize,
    }
    impl Gate for PanicGate {
        fn acquire(&self, _: u32) {
            assert_ne!(
                self.calls.fetch_add(1, Ordering::Relaxed),
                self.trigger,
                "injected worker failure"
            );
        }
        fn current_rate(&self) -> f64 {
            f64::INFINITY
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let cfg = config(temp.path());
    let opens = cfg
        .root
        .as_bytes()
        .split(|b| *b == b'/')
        .filter(|p| !p.is_empty())
        .count()
        + 1;
    let gate = PanicGate {
        calls: AtomicUsize::new(0),
        trigger: opens + 2,
    };
    let (tx, _rx) = crossbeam_channel::bounded(1);
    assert!(matches!(
        walk(&cfg, &gate, &AtomicBool::new(false), tx),
        Err(WalkError::Limit("worker panicked"))
    ));
}

#[test]
fn notable_cutoff_ties_across_reverse_created_directories() {
    let temp = tempfile::tempdir().unwrap();
    // Reverse creation exercises discovery order opposite to final byte preorder.
    // Every tie must be emitted, so the assertion covers every worker assignment.
    let mut expected = BTreeMap::new();
    for i in (0..32).rev() {
        let dir = temp.path().join(format!("a{i:03}"));
        fs::create_dir(&dir).unwrap();
        let file = dir.join("same");
        fs::write(&file, []).unwrap();
        let m = fs::symlink_metadata(file).unwrap();
        expected.insert(
            RawPath::from_path(&dir).unwrap(),
            (m.ino(), m.blocks() * 512),
        );
    }
    assert!(expected
        .values()
        .all(|(_, disk)| *disk == expected.values().next().unwrap().1));
    for threads in [1, 2, 4, 8] {
        let mut cfg = config(temp.path());
        cfg.threads = threads;
        cfg.top_files = 1;
        cfg.big_file_threshold = u64::MAX;
        let (s, rows) = collect(&cfg);
        assert_eq!((s.errors, s.races), (0, 0));
        let records = paths(rows, &cfg.root);
        for (dir, &(ino, disk)) in &expected {
            assert!(records[dir]
                .notable
                .iter()
                .any(|f| (f.ino, f.disk) == (ino, disk)));
        }
        // In particular the first final directory's inode is a global top-1 winner.
        assert_eq!(records[expected.keys().next().unwrap()].notable.len(), 1);
    }
}

#[test]
fn notable_cutoff_ties_preserve_same_directory_inode_winners() {
    let temp = tempfile::tempdir().unwrap();
    let mut expected = Vec::new();
    for i in (0..2048).rev() {
        let path = temp.path().join(format!("f{i:04}"));
        fs::write(&path, []).unwrap();
        let m = fs::symlink_metadata(path).unwrap();
        expected.push((m.ino(), m.blocks() * 512));
    }
    assert!(expected.iter().all(|(_, disk)| *disk == expected[0].1));
    expected.sort_unstable();
    for threads in [1, 2, 4, 8] {
        for top_files in [1, 7] {
            let mut cfg = config(temp.path());
            cfg.threads = threads;
            cfg.top_files = top_files;
            cfg.big_file_threshold = u64::MAX;
            let (s, rows) = collect(&cfg);
            assert_eq!((s.errors, s.races), (0, 0));
            let records = paths(rows, &cfg.root);
            let notable = &records[&cfg.root].notable;
            assert_eq!(notable.len(), expected.len());
            for &(ino, disk) in expected.iter().take(top_files) {
                assert!(notable.iter().any(|f| (f.ino, f.disk) == (ino, disk)));
            }
        }
    }
}
