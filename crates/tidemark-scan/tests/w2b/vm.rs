//! Owned by: W2-B
//! Disposable VM-only XFS equivalence, feature refusal, and scale smoke tests.
use super::*;
use std::{fs::File, os::fd::AsFd, time::Instant};
use test_support::xfs::LoopXfs;
fn walk_sink(root: RawPath) -> VecSink {
    let cfg = config(root.clone());
    let mut agg = aggregator(root);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (tx, rx) = crossbeam_channel::bounded(2);
        let handle = scope.spawn(|| walk(&cfg, &NoopGate, &cancel, tx));
        for out in rx {
            match out {
                WalkOutput::Dir(d) => agg.add_dir_record(d),
                WalkOutput::Gone(g) => agg.add_gone(&g),
                WalkOutput::Ns(_) => panic!("walk returned namespace"),
            }
        }
        handle.join().unwrap().unwrap();
    });
    let mut sink = VecSink::default();
    agg.prepare().unwrap().write(&mut sink).unwrap();
    sink
}
fn bulk_sink(root: RawPath, mount: &std::path::Path) -> (VecSink, BulkStats) {
    let fd = File::open(mount).unwrap();
    let geo = tidemark_sys::xfs::geometry(fd.as_fd()).unwrap();
    let params = BulkParams {
        walk: config(root.clone()),
        batch: 4096,
        threads: 2,
    };
    let (prepared, stats) = run_bulk(
        &params,
        fd.as_fd(),
        &geo,
        &NoopGate,
        &AtomicBool::new(false),
        &|_| {},
        aggregator(root),
    )
    .unwrap();
    let mut sink = VecSink::default();
    prepared.write(&mut sink).unwrap();
    (sink, stats)
}
#[test]
fn bulk_equals_walk_mandatory_fixtures() {
    for fixture in [
        "link_semantics_basic",
        "hardlink_web",
        "sparse",
        "nonutf8",
        "symlink_loops",
        "mixed_special",
        "mixed_special_root",
        "deep",
        "wide_dir",
    ] {
        let source = test_support::materialize_file(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../test-support/fixtures/{fixture}.toml")),
        );
        let fs = LoopXfs::new(2 * 1024 * 1024 * 1024, &[], "");
        // One cp invocation preserves links between root and outside.
        assert!(std::process::Command::new("cp")
            .arg("-a")
            .arg("--")
            .arg(&source.root)
            .arg(&source.outside)
            .arg(&fs.mountpoint)
            .status()
            .unwrap()
            .success());
        let root =
            RawPath::from_path(&fs.mountpoint.join(source.root.file_name().unwrap())).unwrap();
        let mut expected = walk_sink(root.clone());
        let (mut actual, stats) = bulk_sink(root, &fs.mountpoint);
        assert_eq!(stats.races, 0, "fixture {fixture}");
        assert_eq!(stats.names_missing, 0, "fixture {fixture}");
        compare(&mut expected, &mut actual);
    }
}
#[test]
fn bulk_refuses_ftype_zero() {
    let fs = LoopXfs::new(1024 * 1024 * 1024, &["-m", "crc=0", "-n", "ftype=0"], "");
    let fd = File::open(&fs.mountpoint).unwrap();
    let geo = tidemark_sys::xfs::geometry(fd.as_fd()).unwrap();
    assert_eq!(geo.ftype, Some(false));
    let root = RawPath::from_path(&fs.mountpoint).unwrap();
    let params = BulkParams {
        walk: config(root.clone()),
        batch: 4096,
        threads: 2,
    };
    assert!(matches!(
        run_bulk(
            &params,
            fd.as_fd(),
            &geo,
            &NoopGate,
            &AtomicBool::new(false),
            &|_| {},
            aggregator(root)
        ),
        Err(BulkError::Unsupported(_))
    ));
}
#[test]
#[ignore = "scale"]
fn bulk_equals_walk_million_file_timing_smoke() {
    let fs = LoopXfs::new(8 * 1024 * 1024 * 1024, &[], "");
    for d in 0..100 {
        let dir = fs.mountpoint.join(format!("d{d:03}"));
        std::fs::create_dir(&dir).unwrap();
        for f in 0..10_000 {
            File::create(dir.join(format!("f{f:05}"))).unwrap();
        }
    }
    // A fresh copy of this already compiled test executable measures bulk alone.
    // The parent owns/cleans the fixture; the child never mounts or compiles.
    let measured = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "vm::bulk_memory_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TIDEMARK_BULK_MEMORY_ROOT", &fs.mountpoint)
        .env("TIDEMARK_BULK_MEMORY_DIRS", "101")
        .env("TIDEMARK_BULK_MEMORY_FILES", "1000000")
        .output()
        .unwrap();
    eprintln!("{}", String::from_utf8_lossy(&measured.stdout));
    eprintln!("{}", String::from_utf8_lossy(&measured.stderr));
    assert!(
        measured.status.success(),
        "isolated bulk memory child failed"
    );
    let root = RawPath::from_path(&fs.mountpoint).unwrap();
    let start = Instant::now();
    let mut expected = walk_sink(root.clone());
    let walk_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let (mut actual, stats) = bulk_sink(root, &fs.mountpoint);
    let bulk_ms = start.elapsed().as_millis();
    compare(&mut expected, &mut actual);
    // The separate child above measures RSS; this process measures equivalence/timing.
    eprintln!(
        "walk_ms={walk_ms} bulk_ms={bulk_ms} phases={:?}",
        stats.phase_ms
    );
}

// This test is also enumerated normally, when it does no work without the parent seam.
#[test]
fn bulk_memory_child() {
    use std::io::Read;
    let Some(mount) = std::env::var_os("TIDEMARK_BULK_MEMORY_ROOT") else {
        return;
    };
    let dirs: u64 = std::env::var("TIDEMARK_BULK_MEMORY_DIRS")
        .unwrap()
        .parse()
        .unwrap();
    let files: u64 = std::env::var("TIDEMARK_BULK_MEMORY_FILES")
        .unwrap()
        .parse()
        .unwrap();
    let mount = std::path::PathBuf::from(mount);
    let root = RawPath::from_path(&mount).unwrap();
    let fd = File::open(&mount).unwrap();
    let fs = rustix::fs::fstatvfs(&fd).unwrap();
    let used_inodes = fs.f_files.saturating_sub(fs.f_ffree);
    let estimate = estimate_memory(used_inodes, dirs);
    let geo = tidemark_sys::xfs::geometry(fd.as_fd()).unwrap();
    let params = BulkParams {
        walk: config(root.clone()),
        batch: 4096,
        threads: 2,
    };
    let (prepared, _) = run_bulk(
        &params,
        fd.as_fd(),
        &geo,
        &NoopGate,
        &AtomicBool::new(false),
        &|_| {},
        aggregator(root),
    )
    .unwrap();
    let summary = prepared.write(&mut DiscardRows).unwrap();
    assert_eq!(summary.dirs, dirs);
    assert_eq!(summary.files, files);
    let mut status = String::new();
    File::open("/proc/self/status")
        .unwrap()
        .take(65_536)
        .read_to_string(&mut status)
        .unwrap();
    let peak = status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")
                .and_then(|rest| rest.split_whitespace().next())
                .map(|kb| kb.parse::<u64>().unwrap() * 1024)
        })
        .unwrap();
    eprintln!("bulk-only dirs={dirs} files={files} used_inodes={used_inodes} estimate_bytes={estimate} peak_rss_bytes={peak}");
    assert!(u128::from(peak) * 100 <= u128::from(estimate) * 120,
        "bulk peak RSS {peak} exceeds 120% of estimate {estimate}; dirs={dirs} files={files} used_inodes={used_inodes}");
}

struct DiscardRows;
impl agg::RowSink for DiscardRows {
    fn dirs(&mut self, _: &[tidemark_core::snapshot::DirRow]) -> Result<(), agg::SinkError> {
        Ok(())
    }
    fn files(&mut self, _: &[tidemark_core::snapshot::FileRow]) -> Result<(), agg::SinkError> {
        Ok(())
    }
    fn link_sets(
        &mut self,
        _: &[tidemark_core::snapshot::LinkSetRow],
    ) -> Result<(), agg::SinkError> {
        Ok(())
    }
    fn link_instances(
        &mut self,
        _: &[tidemark_core::snapshot::LinkInstanceRow],
    ) -> Result<(), agg::SinkError> {
        Ok(())
    }
    fn skipped(&mut self, _: &[tidemark_core::snapshot::SkipRow]) -> Result<(), agg::SinkError> {
        Ok(())
    }
    fn errors(&mut self, _: &[tidemark_core::snapshot::ErrorRow]) -> Result<(), agg::SinkError> {
        Ok(())
    }
    fn annotations(
        &mut self,
        _: &[tidemark_core::snapshot::AnnotationRow],
    ) -> Result<(), agg::SinkError> {
        Ok(())
    }
}
