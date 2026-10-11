//! Owned by: W5-A
//! Disposable-VM fanotify, exclusion, namespace and load fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    os::{fd::AsFd, unix::fs::MetadataExt},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tidemark_core::path::RawPath;
use tidemark_fs::attribution::AttributionCache;
use tidemark_live::watch::{HotScope, Watcher, WatcherOptions};
use tidemark_sys::fanotify::{mask, Fanotify, InitMode, RawEvent};
fn options(mount: &std::path::Path) -> WatcherOptions {
    WatcherOptions {
        mount: RawPath::from_path(mount).unwrap(),
        events: vec!["close_write".into()],
        hot_depth: 6,
        exclude_prefixes: vec![],
        lru_entries: 200_000,
    }
}
fn watcher(cfg: WatcherOptions) -> Watcher {
    Watcher::start(
        cfg,
        Arc::new(AttributionCache::new(Duration::from_secs(60), 10_000)),
    )
    .unwrap()
}
fn child_write(path: &std::path::Path) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "privileged::writer_child", "--test-threads=1"])
        .env("TIDEMARK_W5A_WRITE_PATH", path)
        .spawn()
        .unwrap()
}
#[test]
fn writer_child() {
    let Some(path) = std::env::var_os("TIDEMARK_W5A_WRITE_PATH") else {
        return;
    };
    std::thread::sleep(Duration::from_millis(100));
    std::fs::write(path, vec![0; 5 * 1024 * 1024]).unwrap();
    std::thread::sleep(Duration::from_secs(2));
}

#[test]
fn fanotify_fd_mode_sees_close_write() {
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let group = Fanotify::init(InitMode::FdMode).unwrap();
    group.mark_mount(&fs.mountpoint, mask::CLOSE_WRITE).unwrap();
    let path = fs.mountpoint.join("written");
    let watch = watcher(options(&fs.mountpoint));
    let attribution = AttributionCache::new(Duration::from_secs(60), 10);
    let mut child = child_write(&path);
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut buffer = vec![0; 65536];
    let mut found = false;
    while Instant::now() < deadline && !found {
        for event in group.read(&mut buffer).unwrap() {
            if let RawEvent::Fd { pid, fd, .. } = event.unwrap() {
                let resolved = std::fs::read_link(format!(
                    "/proc/self/fd/{}",
                    std::os::fd::AsRawFd::as_raw_fd(&fd)
                ))
                .unwrap();
                if resolved != path {
                    continue;
                }
                let st = rustix::fs::fstat(fd.as_fd()).unwrap();
                let measured = std::fs::metadata(&path).unwrap();
                assert_eq!(st.st_blocks as u64 * 512, measured.blocks() * 512);
                assert_eq!(st.st_size as u64, 5 * 1024 * 1024);
                assert_eq!(pid as u32, child.id());
                let who = attribution.lookup(pid as u32);
                assert_ne!(who.comm, "?");
                assert_eq!(who.pid, child.id());
                found = true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait().unwrap();
    assert!(found, "close-write not observed in 1 second");
    let rows = watch.hot(5, HotScope::Dir, 5);
    let measured = std::fs::metadata(&path).unwrap().blocks() * 512;
    assert_eq!(rows[0].bytes_delta, measured as i64);
    assert_eq!(rows[0].key, fs.mountpoint.as_os_str().as_encoded_bytes());
    watch.stop();
}
#[test]
fn exclude_prefixes_respected() {
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let skip = fs.mountpoint.join("skip");
    let keep = fs.mountpoint.join("skipper");
    std::fs::create_dir(&skip).unwrap();
    std::fs::create_dir(&keep).unwrap();
    let mut cfg = options(&fs.mountpoint);
    cfg.exclude_prefixes
        .push(RawPath::from_path(&skip).unwrap());
    let watch = watcher(cfg);
    let mut a = child_write(&skip.join("file"));
    let mut b = child_write(&keep.join("file"));
    a.wait().unwrap();
    b.wait().unwrap();
    let rows = watch.hot(5, HotScope::Dir, 100);
    assert!(rows
        .iter()
        .any(|r| r.key == keep.as_os_str().as_encoded_bytes()));
    assert!(!rows
        .iter()
        .any(|r| r.key == skip.as_os_str().as_encoded_bytes()));
    let started = Instant::now();
    watch.stop();
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[test]
fn watcher_namespace_guard() {
    const CHILD: &str = "TIDEMARK_W5A_NAMESPACE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        assert!(!tidemark_live::watch::namespace_ok().unwrap());
        let result = Watcher::start(
            options(std::path::Path::new("/")),
            Arc::new(AttributionCache::new(Duration::from_secs(60), 10)),
        );
        assert!(matches!(
            result,
            Err(tidemark_live::watch::WatchError::Namespace)
        ));
        return;
    }
    let status = Command::new("unshare")
        .arg("-m")
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "privileged::watcher_namespace_guard",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .status()
        .unwrap();
    assert!(status.success());
}
// Kernel overflow through the running watcher is tested in the watch module,
// where the processing lock can pause consumption without a production test API.
#[test]
fn fid_mode_create_delete_rename() {
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let caps = tidemark_sys::fanotify::probe(&fs.mountpoint).unwrap();
    if !caps.report_dfid_name {
        return;
    }
    let group = Fanotify::init(InitMode::DfidName).unwrap();
    group
        .mark_filesystem(
            &fs.mountpoint,
            mask::CREATE | mask::DELETE | mask::MOVED_FROM | mask::MOVED_TO | mask::ONDIR,
        )
        .unwrap();
    let from = fs.mountpoint.join("from");
    let to = fs.mountpoint.join("to");
    std::fs::write(&from, b"fixture").unwrap();
    std::fs::rename(&from, &to).unwrap();
    std::fs::remove_file(&to).unwrap();
    let mut buffer = vec![0; 65536];
    let mut seen = 0;
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        for event in group.read(&mut buffer).unwrap() {
            if let RawEvent::Fid {
                mask,
                handle_type,
                handle,
                name,
                ..
            } = event.unwrap()
            {
                let mount_fd = std::fs::File::open(&fs.mountpoint).unwrap();
                let fd =
                    tidemark_sys::fanotify::open_by_handle(mount_fd.as_fd(), handle_type, &handle)
                        .unwrap();
                let path = std::fs::read_link(format!(
                    "/proc/self/fd/{}",
                    std::os::fd::AsRawFd::as_raw_fd(&fd)
                ))
                .unwrap();
                assert_eq!(path, fs.mountpoint);
                assert!(matches!(name.as_deref(), Some(b"from") | Some(b"to")));
                seen |= mask;
            }
        }
        if seen & (mask::CREATE | mask::DELETE | mask::MOVED_FROM | mask::MOVED_TO)
            == mask::CREATE | mask::DELETE | mask::MOVED_FROM | mask::MOVED_TO
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        seen & (mask::CREATE | mask::DELETE | mask::MOVED_FROM | mask::MOVED_TO),
        mask::CREATE | mask::DELETE | mask::MOVED_FROM | mask::MOVED_TO
    );
}
#[test]
#[ignore = "scale"]
fn watcher_sixty_second_load_and_ten_minute_fd_stability() {
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let tasks = || {
        std::fs::read_dir("/proc/self/task")
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let prior_tasks = tasks();
    let watch = watcher(options(&fs.mountpoint));
    let deadline = Instant::now() + Duration::from_secs(2);
    let tid = loop {
        let current = tasks();
        if let Some(tid) = current.difference(&prior_tasks).find(|tid| {
            std::fs::read_to_string(
                std::path::Path::new("/proc/self/task")
                    .join(tid)
                    .join("comm"),
            )
            .is_ok_and(|name| name.trim() == "tidemark-watch")
        }) {
            break tid.clone();
        }
        assert!(Instant::now() < deadline, "watcher thread not found");
        std::thread::sleep(Duration::from_millis(5));
    };
    let cpu_ticks = || {
        let stat = std::fs::read_to_string(
            std::path::Path::new("/proc/self/task")
                .join(&tid)
                .join("stat"),
        )
        .unwrap();
        let fields: Vec<_> = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .collect();
        fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
    };
    let hz = String::from_utf8(
        Command::new("getconf")
            .arg("CLK_TCK")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .parse::<f64>()
    .unwrap();
    let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let before = count();
    let cpu_before = cpu_ticks();
    let started = Instant::now();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "privileged::paced_writer_child",
            "--test-threads=1",
        ])
        .env("TIDEMARK_W5A_LOAD_PATH", &fs.mountpoint)
        .spawn()
        .unwrap();
    let mut measured = false;
    let mut previous = watch.stats().events_total;
    while started.elapsed() < Duration::from_secs(600) {
        std::thread::sleep(Duration::from_secs(1));
        let current = watch.stats().events_total;
        assert!(current > previous, "watcher stopped processing");
        previous = current;
        assert_ne!(watch.mode(), tidemark_live::watch::WatchMode::Failed);
        if !measured && started.elapsed() >= Duration::from_secs(60) {
            let cpu_pct =
                (cpu_ticks() - cpu_before) as f64 / hz / started.elapsed().as_secs_f64() * 100.0;
            println!(
                "watcher CPU {cpu_pct:.3}% of one core; events={}, elapsed={:?}",
                watch.stats().events_total,
                started.elapsed()
            );
            assert!(cpu_pct < 2.0, "watcher exceeds CPU budget");
            measured = true;
        }
    }
    assert!(child.wait().unwrap().success());
    let achieved = 1_200_000.0 / started.elapsed().as_secs_f64();
    println!("generated close-write rate={achieved:.2}/s");
    assert!(
        achieved >= 1960.0,
        "VM cannot sustain calibrated 2,000 closes/s"
    );
    assert!(measured);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(count(), before);
    watch.stop();
}
#[test]
fn paced_writer_child() {
    let Some(path) = std::env::var_os("TIDEMARK_W5A_LOAD_PATH") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let started = Instant::now();
    for n in 0..1_200_000 {
        let due = started + Duration::from_micros(n * 500);
        if let Some(delay) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
        std::fs::write(path.join((n % 2000).to_string()), b"load").unwrap();
        if n == 119_999 {
            let achieved = 120_000.0 / started.elapsed().as_secs_f64();
            println!("first 60 seconds generated {achieved:.2} closes/s");
            assert!(achieved >= 1960.0, "VM cannot sustain calibrated load");
        }
    }
}
