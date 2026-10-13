//! Owned by: WDAEMON-polish
//! Source-pinned operator message formatting through the real tracing subscriber.
use std::{io::Write, sync::Arc};
#[derive(Clone)]
struct Sink(Arc<parking_lot::Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[test]
fn every_operator_message_formats_with_values_and_without_debug_fields() {
    let bytes = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = Sink(bytes.clone());
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_writer(move || sink.clone())
        .finish();
    let error = "fixture failure";
    let e = "fixture failure";
    let name = "sampler";
    let needed = 1024;
    let mounts = 2;
    let batch = 16;
    let reason = "resource pressure";
    let mode = "walk";
    let attempts = 3;
    let due = 1234;
    let method = "status";
    let uid = 0;
    let pid = 1234;
    let outcome = "ok";
    let millis = 3;
    let path = "/fixture";
    tracing::subscriber::with_default(subscriber, || {
        let watcher_source = include_str!("../../tidemark-live/src/watch/mod.rs");
        assert!(watcher_source.contains("watcher started: mount {}, mode {mode}"));
        assert!(watcher_source.contains("fanotify poll failed: {error}"));
        assert!(watcher_source.contains("fanotify read failed: {error}"));
        let mount = tidemark_core::path::RawPath::new(b"/fixture".to_vec()).unwrap();
        tracing::info!(
            "watcher started: mount {}, mode {mode}",
            mount.display_lossy()
        );
        tracing::warn!("fanotify poll failed: {error}");
        tracing::warn!("fanotify read failed: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("watcher started: mount /fixture, mode walk"));
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("fanotify poll failed: fixture failure"));
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("fanotify read failed: fixture failure"));

        crate::log_descriptor_limit(rustix::process::Rlimit {
            current: Some(65536),
            maximum: Some(65536),
        });
        crate::log_descriptor_limit(rustix::process::Rlimit {
            current: None,
            maximum: None,
        });
        crate::api::log_request(0, 1234, "status", true, std::time::Duration::from_millis(3));
        crate::raise_fd_limit(
            || rustix::process::Rlimit {
                current: Some(1024),
                maximum: Some(65536),
            },
            |_| Err(rustix::io::Errno::PERM),
        );
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("effective descriptor limit: soft 65536, hard 65536"));
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("effective descriptor limit: soft unlimited, hard unlimited"));
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("API request: status from uid 0 pid 1234, ok in 3 ms"));

        assert!(include_str!("firstboot.rs").contains("boot timestamp unavailable: {error}"));
        tracing::warn!("boot timestamp unavailable: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("boot timestamp unavailable: fixture failure"));
        assert!(include_str!("firstboot.rs").contains(
            "Tidemark first-boot report published: healthy {}, version {}, config sha256 {}"
        ));
        tracing::info!(
            "Tidemark first-boot report published: healthy {}, version {}, config sha256 {}",
            true,
            "0.1.0",
            "abcd"
        );
        assert!(String::from_utf8_lossy(&bytes.lock()).contains(
            "Tidemark first-boot report published: healthy true, version 0.1.0, config sha256 abcd"
        ));
        assert!(include_str!("lib.rs").contains("effective descriptor limit: soft {}, hard {}"));
        tracing::info!(
            "effective descriptor limit: soft {}, hard {}",
            "65536",
            "65536"
        );
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("effective descriptor limit: soft 65536, hard 65536"));
        assert!(include_str!("lib.rs").contains("could not raise descriptor soft limit: {error}"));
        tracing::warn!("could not raise descriptor soft limit: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("could not raise descriptor soft limit: fixture failure"));
        assert!(include_str!("lib.rs").contains("descriptor budget reduced; minimum batch may exceed available reserve: soft {}, needed {needed}, mounts {mounts}, batch {batch}"));
        tracing::warn!("descriptor budget reduced; minimum batch may exceed available reserve: soft {}, needed {needed}, mounts {mounts}, batch {batch}" ,"65536");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("descriptor budget reduced; minimum batch may exceed available reserve: soft 65536, needed 1024, mounts 2, batch 16"));
        assert!(
            include_str!("lib.rs").contains("startup resource pressure; retrying {name}: {error}")
        );
        tracing::warn!("startup resource pressure; retrying {name}: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("startup resource pressure; retrying sampler: fixture failure"));
        assert!(include_str!("lib.rs").contains("{reason}"));
        tracing::error!("{reason}");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("resource pressure"));
        assert!(include_str!("lib.rs").contains("daemon ready"));
        tracing::info!("daemon ready");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("daemon ready"));
        assert!(include_str!("lib.rs").contains("config reload: accepted"));
        tracing::info!("config reload: accepted");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("config reload: accepted"));
        assert!(include_str!("lib.rs").contains("config reload rejected: {e}"));
        tracing::error!("config reload rejected: {e}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("config reload rejected: fixture failure"));
        assert!(include_str!("lib.rs").contains("firstboot followup failed: {error}"));
        tracing::warn!("firstboot followup failed: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("firstboot followup failed: fixture failure"));
        assert!(include_str!("lib.rs").contains("daemon stopping"));
        tracing::info!("daemon stopping");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("daemon stopping"));
        assert!(include_str!("lib.rs").contains("service implementation pending: {name}"));
        tracing::warn!("service implementation pending: {name}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("service implementation pending: sampler"));
        assert!(include_str!("lib.rs").contains("service degraded at shutdown: {name}: {error}"));
        tracing::warn!("service degraded at shutdown: {name}: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("service degraded at shutdown: sampler: fixture failure"));
        assert!(
            include_str!("lib.rs").contains("startup scan cleanup skipped: host scan lock is held")
        );
        tracing::info!("startup scan cleanup skipped: host scan lock is held");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("startup scan cleanup skipped: host scan lock is held"));
        assert!(include_str!("lib.rs").contains("keeping previous config: reload rejected: {e}"));
        tracing::error!("keeping previous config: reload rejected: {e}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("keeping previous config: reload rejected: fixture failure"));
        assert!(include_str!("metrics.rs").contains("metrics exporter degraded: {error}"));
        tracing::warn!("metrics exporter degraded: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("metrics exporter degraded: fixture failure"));
        assert!(include_str!("mounts.rs").contains("mount not present: {path}"));
        tracing::warn!("mount not present: {path}");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("mount not present: /fixture"));
        assert!(include_str!("mounts.rs").contains("mount present: {path} ({}:{})"));
        tracing::info!("mount present: {path} ({}:{})", 8, 1);
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("mount present: /fixture (8:1)"));
        assert!(include_str!("mounts.rs").contains("mount changed: {path}"));
        tracing::warn!("mount changed: {path}");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("mount changed: /fixture"));
        assert!(include_str!("scheduler.rs").contains("scan started: root {}, scan id {}"));
        tracing::info!("scan started: root {}, scan id {}", "/fixture", "scan-1");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("scan started: root /fixture, scan id scan-1"));
        assert!(include_str!("scheduler.rs")
            .contains("post-scan publication incomplete; dirty state retained: {error}"));
        tracing::warn!("post-scan publication incomplete; dirty state retained: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("post-scan publication incomplete; dirty state retained: fixture failure"));
        assert!(include_str!("scheduler.rs").contains("scan failed: root {}, mode {mode}, source {}, attempt {attempts}, retry timestamp {due}: {e}"));
        tracing::warn!("scan failed: root {}, mode {mode}, source {}, attempt {attempts}, retry timestamp {due}: {e}" ,"/fixture", "scheduled");
        assert!(String::from_utf8_lossy(&bytes.lock()).contains("scan failed: root /fixture, mode walk, source scheduled, attempt 3, retry timestamp 1234: fixture failure"));
        assert!(include_str!("scheduler.rs").contains("scan completed: root {}, scan id {}"));
        tracing::info!("scan completed: root {}, scan id {}", "/fixture", "scan-1");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("scan completed: root /fixture, scan id scan-1"));
        assert!(include_str!("api.rs")
            .contains("API request: {method} from uid {uid} pid {pid}, {outcome} in {millis} ms"));
        tracing::info!("API request: {method} from uid {uid} pid {pid}, {outcome} in {millis} ms");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("API request: status from uid 0 pid 1234, ok in 3 ms"));
        assert!(include_str!("api.rs").contains("API socket cleanup failed: {error}"));
        tracing::warn!("API socket cleanup failed: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("API socket cleanup failed: fixture failure"));
        assert!(include_str!("api.rs").contains("API lock cleanup failed: {error}"));
        tracing::warn!("API lock cleanup failed: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("API lock cleanup failed: fixture failure"));
        assert!(include_str!("api.rs").contains("API accept degraded; retrying: {error}"));
        tracing::warn!("API accept degraded; retrying: {error}");
        assert!(String::from_utf8_lossy(&bytes.lock())
            .contains("API accept degraded; retrying: fixture failure"));
    });
    let output = String::from_utf8(bytes.lock().clone()).unwrap();
    assert!(!output.contains("Some("));
    assert!(!output.contains('"'));
}
#[test]
fn daemon_log_calls_do_not_use_debug_fields() {
    fn visit(path: &std::path::Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("log_tests.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for prefix in ["tracing::info!(", "tracing::warn!(", "tracing::error!("] {
                for call in source.split(prefix).skip(1) {
                    let fields = call.split('"').next().unwrap();
                    assert!(
                        !fields.contains('?'),
                        "debug field in {}: {fields}",
                        path.display()
                    );
                }
            }
        }
    }
    visit(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .as_path(),
    );
}
