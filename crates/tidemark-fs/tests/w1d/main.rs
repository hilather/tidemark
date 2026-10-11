//! Owned by: W1-D
//! Parser, throttle and metadata-only procfs integration tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::{dev::DiskStat, scan::Gate};
use tidemark_fs::{procfs::*, throttle::*};

fn params() -> AdaptiveParams {
    AdaptiveParams {
        max_ops: 1000,
        min_ops: 100,
        max_util_pct: 50.0,
        target_latency_ms: 15.0,
        interval: Duration::from_millis(20),
    }
}
#[test]
fn parser_samples() {
    assert_eq!(
        parse_cgroup_unit(include_str!("data/cgroup-v1.txt")),
        Some("postgresql.service".into())
    );
    assert_eq!(
        parse_cgroup_unit(include_str!("data/cgroup-v2.txt")),
        Some("user-1000".into())
    );
    assert_eq!(
        parse_cgroup_unit("2:cpu:/unrelated.service\n0::/system.slice/a.service/child.scope"),
        Some("child.scope".into())
    );
    assert_eq!(parse_cgroup_unit("0::/"), None);
    let flags = parse_fdinfo_flags(include_str!("data/fdinfo.txt")).unwrap();
    assert_eq!(flags & libc::O_ACCMODE as u32, libc::O_WRONLY as u32);
    assert_ne!(flags & libc::O_APPEND as u32, 0);
    assert_eq!(parse_fdinfo_flags("flags: 089"), None);
    let stat =
        "42 (a comm ) with (parentheses)) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 123456 0";
    assert_eq!(parse_stat_starttime(stat), Some(123456));
    assert_eq!(parse_stat_starttime("42 (short) S 1"), None);
    assert_eq!(parse_stat_starttime(""), None);
    let captured = include_str!("data/stat.txt");
    let expected = captured
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(parse_stat_starttime(captured), Some(expected));
    assert_eq!(
        parse_cgroup_unit("malformed\n1:name=systemd:/system.slice/a.service"),
        Some("a.service".into())
    );
    assert!(read_starttime(std::process::id()).unwrap() > 0);
    assert!(!read_comm(std::process::id()).unwrap().is_empty());
    assert!(read_uid(std::process::id()).is_ok());
}
#[test]
fn controller_table() {
    let p = params();
    let idle = DiskStat::default();
    for (stat, expected) in [
        (idle.clone(), 550.0),
        (
            DiskStat {
                io_ticks_ms: 300,
                ..idle.clone()
            },
            250.0,
        ),
        (
            DiskStat {
                read_ios: 10,
                read_ticks_ms: 160,
                ..idle.clone()
            },
            250.0,
        ),
        (
            DiskStat {
                write_ios: 2,
                write_ticks_ms: 32,
                ..idle.clone()
            },
            250.0,
        ),
        (
            DiskStat {
                read_ios: 10,
                read_ticks_ms: 150,
                io_ticks_ms: 250,
                ..idle.clone()
            },
            550.0,
        ),
    ] {
        assert_eq!(
            controller_step(500.0, std::slice::from_ref(&idle), &[stat], 500.0, &p),
            expected
        );
    }
    assert_eq!(
        controller_step(
            100.0,
            std::slice::from_ref(&idle),
            &[DiskStat {
                io_ticks_ms: 500,
                ..idle.clone()
            }],
            500.0,
            &p
        ),
        100.0
    );
    assert_eq!(
        controller_step(
            990.0,
            std::slice::from_ref(&idle),
            std::slice::from_ref(&idle),
            500.0,
            &p
        ),
        1000.0
    );
    assert_eq!(
        controller_step(
            500.0,
            &[idle.clone(), idle.clone()],
            &[
                idle.clone(),
                DiskStat {
                    io_ticks_ms: 500,
                    ..idle.clone()
                }
            ],
            500.0,
            &p
        ),
        250.0
    );
    assert_eq!(
        controller_step(
            500.0,
            &[DiskStat {
                read_ios: 1,
                ..idle.clone()
            }],
            std::slice::from_ref(&idle),
            500.0,
            &p
        ),
        500.0
    );
    for dt in [0.0, -1.0, f64::NAN] {
        assert_eq!(
            controller_step(
                500.0,
                std::slice::from_ref(&idle),
                std::slice::from_ref(&idle),
                dt,
                &p
            ),
            500.0
        );
    }
    assert_eq!(controller_step(500.0, &[], &[], 500.0, &p), 500.0);
}
#[test]
fn four_thread_fixed_rate() {
    let gate = Arc::new(FixedGate::new(1000.0));
    // Spend the documented initial 250ms burst before measuring steady state.
    gate.acquire(250);
    let start = Instant::now();
    let count = Arc::new(AtomicU64::new(0));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let gate = Arc::clone(&gate);
            let count = Arc::clone(&count);
            std::thread::spawn(move || {
                while start.elapsed() < Duration::from_secs(2) {
                    gate.acquire(1);
                    count.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let count = count.load(Ordering::Relaxed);
    assert!((1900..=2100).contains(&count), "count {count}");
}
#[test]
fn controller_stop_drop_and_sampler_failure() {
    let calls = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&calls);
    let mut p = params();
    p.interval = Duration::from_secs(10);
    let gate = AdaptiveGate::start(
        p,
        Box::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
            Err(std::io::Error::other("sample failure"))
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(1);
    while calls.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(gate.current_rate(), 1000.0);
    let start = Instant::now();
    gate.stop();
    gate.stop();
    assert!(start.elapsed() < Duration::from_millis(100));
    assert!((gate.stats().mean_rate - 1000.0).abs() < 1e-9);
    let before = calls.load(Ordering::Relaxed);
    drop(gate);
    assert_eq!(calls.load(Ordering::Relaxed), before);
    let gate = AdaptiveGate::start(params(), Box::new(|| Ok(vec![DiskStat::default()])));
    drop(gate); // Last drop joins without an explicit stop.
}
#[test]
fn config_profiles_and_fallback() {
    let mut cfg = tidemark_core::config::ThrottleConfig::default();
    cfg.profiles.insert("custom".into(), Default::default());
    assert!(
        gate_from_config("max", &cfg, &[], Box::new(|| panic!("max must not sample")))
            .unwrap()
            .current_rate()
            .is_infinite()
    );
    assert_eq!(
        gate_from_config(
            "custom",
            &cfg,
            &[],
            Box::new(|| panic!("fallback must not sample"))
        )
        .unwrap()
        .current_rate(),
        f64::from(cfg.min_ops_per_sec)
    );
    assert!(gate_from_config("missing", &cfg, &[], Box::new(|| Ok(vec![]))).is_err());
}
#[test]
fn finds_deleted_open_file_without_reading_contents() {
    use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(12345).unwrap();
    let meta = file.as_file().metadata().unwrap();
    let file = file.into_file(); // unlinks the pathname, retains the descriptor
    let found = scan_open_files(&[meta.dev()], true).unwrap();
    let found = found
        .iter()
        .find(|f| f.pid == std::process::id() && f.fd == file.as_raw_fd() as u32)
        .unwrap();
    assert_eq!(found.nlink, 0);
    assert_eq!(found.size, 12345);
    assert_eq!(found.ino, meta.ino());
    assert!(found.target.ends_with(b" (deleted)"));
    assert!(found.flags.is_some());
    assert!(scan_open_files(&[], false).unwrap().is_empty());
}
#[test]
fn exiting_processes_are_tolerated() {
    use std::os::unix::fs::MetadataExt;
    let directory = tempfile::tempdir().unwrap();
    let dev = directory.path().metadata().unwrap().dev();
    let worker = std::thread::spawn(|| {
        for _ in 0..20 {
            std::process::Command::new("true").status().unwrap();
        }
    });
    for _ in 0..5 {
        scan_open_files(&[dev], false).unwrap();
    }
    worker.join().unwrap();
}

#[cfg(feature = "privileged")]
#[test]
fn root_sees_other_user_open_file() {
    assert_eq!(
        read_uid(std::process::id()).unwrap(),
        0,
        "requires root in VM"
    );
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::fs::{MetadataExt, PermissionsExt},
        process::{Command, Stdio},
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let meta = file.as_file().metadata().unwrap();
    let mut child = Command::new("setpriv").args(["--reuid=65534", "--regid=65534", "--clear-groups", "python3", "-c", "import os,sys; f=open(sys.argv[1]); print(os.getpid(),f.fileno(),flush=True); sys.stdin.readline()"])
        .arg(file.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let ids: Vec<u32> = line
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    let found = scan_open_files(&[meta.dev()], false).unwrap();
    child.stdin.take().unwrap().write_all(b"done\n").unwrap();
    assert!(child.wait().unwrap().success());
    assert!(found
        .iter()
        .any(|f| f.pid == ids[0] && f.fd == ids[1] && f.ino == meta.ino()));
}

#[test]
fn adaptive_rates_stats_and_stop_during_sampling() {
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let mut count = 0;
    let mut p = params();
    p.interval = Duration::from_millis(200);
    let gate = AdaptiveGate::start(
        p,
        Box::new(move || {
            count += 1;
            if count == 2 {
                sent.send(()).unwrap();
                wait.recv().unwrap();
            }
            Ok(vec![DiskStat {
                io_ticks_ms: count * 500,
                ..DiskStat::default()
            }])
        }),
    );
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    let clone = Arc::clone(&gate);
    let stopped = std::thread::spawn(move || clone.stop());
    let before = Instant::now();
    release.send(()).unwrap();
    stopped.join().unwrap();
    assert!(before.elapsed() < Duration::from_millis(200));
    assert_eq!(gate.current_rate(), 500.0);
    let stats = gate.stats();
    assert_eq!(stats.min_rate, 500.0);
    assert_eq!(stats.decreases, 1);
    assert!((500.0..=1000.0).contains(&stats.mean_rate));
}

#[test]
fn nonrotational_util_is_ignored_by_config_gate() {
    use tidemark_core::dev::{BlockDevInfo, DevKind};
    let leaf = BlockDevInfo {
        name: "synthetic".into(),
        major: 0,
        minor: 0,
        kind: DevKind::Disk,
        rotational: Some(false),
        scheduler: None,
    };
    let cfg = tidemark_core::config::ThrottleConfig {
        sample_interval: Duration::from_millis(5),
        ..Default::default()
    };
    let samples = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&samples);
    let gate = gate_from_config(
        "normal",
        &cfg,
        &[leaf],
        Box::new(move || {
            let sample = counter.fetch_add(1, Ordering::Relaxed);
            Ok(vec![DiskStat {
                io_ticks_ms: sample * 10000,
                ..DiskStat::default()
            }])
        }),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while samples.load(Ordering::Relaxed) < 3 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(gate.current_rate(), f64::from(cfg.max_ops_per_sec));
    drop(gate);
}
