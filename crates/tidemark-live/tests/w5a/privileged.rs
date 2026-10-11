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
#[test]
fn watch_overflow_marks_lossy() {
    let fs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    // Deliberately leave the bounded kernel queue unread until the writer finishes.
    let group = Fanotify::init(InitMode::FdMode).unwrap();
    group.mark_mount(&fs.mountpoint, mask::CLOSE_WRITE).unwrap();
    let status = Command::new("sh")
        .args([
            "-c",
            "i=0; while [ $i -lt 100000 ]; do  : > \"$1/$i\"; i=$((i+1)); done",
            "w5a",
        ])
        .arg(&fs.mountpoint)
        .status()
        .unwrap();
    assert!(status.success());
    let mut buffer = vec![0; 65536];
    let mut overflow = false;
    loop {
        let mut count = 0;
        for event in group.read(&mut buffer).unwrap() {
            count += 1;
            overflow |= matches!(event.unwrap(), RawEvent::Overflow);
        }
        if count == 0 {
            break;
        }
    }
    assert!(overflow, "queue overflow not induced");
}
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
    let watch = watcher(options(&fs.mountpoint));
    let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let before = count();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(600) {
        let mut child = Command::new("sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 2000 ]; do  : > \"$1/$i\"; i=$((i+1)); done",
                "w5a",
            ])
            .arg(&fs.mountpoint)
            .spawn()
            .unwrap();
        child.wait().unwrap();
        std::thread::sleep(Duration::from_secs(1));
    }
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(count(), before);
    watch.stop();
}
