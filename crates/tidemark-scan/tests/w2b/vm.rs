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
    let root = RawPath::from_path(&fs.mountpoint).unwrap();
    let start = Instant::now();
    let mut expected = walk_sink(root.clone());
    let walk_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let (mut actual, stats) = bulk_sink(root, &fs.mountpoint);
    let bulk_ms = start.elapsed().as_millis();
    compare(&mut expected, &mut actual);
    // Timing/equivalence only. Isolated bulk RSS acceptance remains pending VM.
    eprintln!(
        "walk_ms={walk_ms} bulk_ms={bulk_ms} phases={:?}",
        stats.phase_ms
    );
}
