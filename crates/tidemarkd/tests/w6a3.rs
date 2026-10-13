//! Owned by: W6-A3
//! API dispatch, framing, admission and shutdown evidence.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tidemark_core::{api::*, config::Config};
use tidemarkd::{
    api::{self, Context},
    scheduler::ScanControl,
    state::LiveState,
};
const METHODS: &[(&str, &str)] = &[
    ("status", "{}"),
    ("now", "{}"),
    ("hot", r#"{"window":"5m","by":"dir","limit":20}"#),
    ("lvm", "{}"),
    ("phantom", "{}"),
    ("writers", r#"{"window":"2m"}"#),
    ("alerts.active", "{}"),
    ("alerts.history", "{}"),
    ("scan.start", r#"{"root":"/fixture"}"#),
    ("scan.status", "{}"),
    ("scan.cancel", r#"{"root":"/fixture"}"#),
    ("config.reload", "{}"),
];
fn context(c: &Config) -> Context {
    Context {
        state: LiveState::new(c, 10),
        scans: ScanControl::default(),
        reload: Arc::new(|| {
            Ok(ConfigReloadResult {
                reloaded: true,
                warnings: Vec::new(),
            })
        }),
    }
}
fn request(method: &str, params: &str) -> Request {
    Request {
        v: 1,
        id: "test".into(),
        method: method.into(),
        params: serde_json::from_str(params).unwrap(),
    }
}
#[test]
fn every_method_authorizes_before_dispatch_and_validates_parameters() {
    let ctx = context(&Config::default());
    let own = 0; // Valid requests use root even on a non-root test host.
    let other = if own == 1 { 2 } else { 1 };
    for &(method, params) in METHODS {
        let r = request(method, params);
        assert_eq!(
            api::dispatch(&r, other, &ctx, 20)
                .unwrap()
                .error
                .unwrap()
                .code,
            ApiErrorCode::Forbidden,
            "{method}"
        );
        {
            let uid = 0;
            let response = api::dispatch(&r, uid, &ctx, 20).unwrap();
            assert_ne!(
                response.error.as_ref().map(|e| e.code),
                Some(ApiErrorCode::Forbidden),
                "{method}"
            );
            assert_ne!(
                response.error.as_ref().map(|e| e.code),
                Some(ApiErrorCode::BadRequest),
                "{method}"
            );
        }
        let bad = api::dispatch(&request(method, r#"{"unexpected":true}"#), own, &ctx, 20).unwrap();
        assert_eq!(
            bad.error.unwrap().code,
            ApiErrorCode::BadRequest,
            "{method}"
        );
    }
    let r = api::dispatch(&request("not.documented", "{}"), own, &ctx, 20).unwrap();
    assert_eq!(r.error.unwrap().code, ApiErrorCode::BadRequest);
    let mut r = request("status", "{}");
    r.v = 2;
    assert_eq!(
        api::dispatch(&r, own, &ctx, 20)
            .unwrap()
            .error
            .unwrap()
            .code,
        ApiErrorCode::BadRequest
    );
}
fn socket_config(dir: &tempfile::TempDir) -> Config {
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut c = Config::default();
    c.general.socket = dir.path().join("api.sock");
    c
}
fn read(stream: &mut BufReader<UnixStream>) -> Response {
    let mut s = String::new();
    stream.read_line(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}
fn write(stream: &mut BufReader<UnixStream>, r: &Request) {
    serde_json::to_writer(stream.get_mut(), r).unwrap();
    stream.get_mut().write_all(b"\n").unwrap();
}
#[test]
fn framing_multiple_requests_private_socket_and_idle_shutdown() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(&c.general.socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let stream = UnixStream::connect(&c.general.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    stream.get_mut().write_all(b"{malformed}\n").unwrap();
    assert_eq!(
        read(&mut stream).error.unwrap().code,
        ApiErrorCode::BadRequest
    );
    let bytes = serde_json::to_vec(&request("status", "{}")).unwrap();
    stream.get_mut().write_all(&bytes[..5]).unwrap();
    stream.get_mut().write_all(&bytes[5..]).unwrap();
    stream.get_mut().write_all(b"\n").unwrap();
    assert!(read(&mut stream).ok);
    write(&mut stream, &request("config.reload", "{}"));
    assert!(read(&mut stream).ok);
    stream
        .get_mut()
        .write_all(&vec![b'x'; 1024 * 1024])
        .unwrap();
    stream.get_mut().write_all(b"\n").unwrap();
    assert_eq!(
        read(&mut stream).error.unwrap().code,
        ApiErrorCode::BadRequest
    );
    let _idle = UnixStream::connect(&c.general.socket).unwrap();
    let began = std::time::Instant::now();
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(!c.general.socket.exists());
}
#[test]
fn controllable_handler_timeout_keeps_admission_bounded() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
    let (release_tx, release_rx) = crossbeam_channel::bounded(1);
    let mut ctx = context(&c);
    ctx.reload = Arc::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        Ok(ConfigReloadResult {
            reloaded: true,
            warnings: vec![],
        })
    });
    let handle = api::start_with_peer_timeout(
        Arc::new(c.clone()),
        ctx,
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
        Duration::from_millis(50),
    )
    .unwrap();
    let stream = UnixStream::connect(&c.general.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    write(&mut stream, &request("config.reload", "{}"));
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        read(&mut stream).error.unwrap().code,
        ApiErrorCode::Unavailable
    );
    stop.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(80));
    assert!(
        !handle.is_finished(),
        "service abandoned its blocked callback"
    );
    release_tx.send(()).unwrap();
    handle.join().unwrap().unwrap();
}
#[test]
fn refuses_non_socket_and_active_socket_removes_stale_socket() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    std::fs::write(&c.general.socket, b"do not remove").unwrap();
    assert!(api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1)))
    )
    .is_err());
    assert_eq!(std::fs::read(&c.general.socket).unwrap(), b"do not remove");
    std::fs::remove_file(&c.general.socket).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&c.general.socket).unwrap();
    assert!(api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1)))
    )
    .is_err());
    drop(listener);
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
    )
    .unwrap();
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}
#[test]
fn thirty_two_connections_then_rejects_without_queueing() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = crossbeam_channel::bounded(32);
    let handle = api::start_with_peer_timeout(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(move |_| {
            tx.send(()).unwrap();
            Ok((0, 1))
        }),
        Duration::from_millis(700),
    )
    .unwrap();
    let mut clients = Vec::new();
    for _ in 0..32 {
        clients.push(UnixStream::connect(&c.general.socket).unwrap());
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    let mut rejected = UnixStream::connect(&c.general.socket).unwrap();
    rejected
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(std::io::Read::read(&mut rejected, &mut byte).unwrap(), 0);
    // All idle slots must expire; credentials on a fresh connection prove admission.
    std::thread::sleep(Duration::from_millis(850));
    let mut recovered = BufReader::new(UnixStream::connect(&c.general.socket).unwrap());
    recovered
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    write(&mut recovered, &request("status", "{}"));
    assert!(read(&mut recovered).ok);
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}
#[cfg(feature = "privileged")]
#[path = "w6a3/privileged.rs"]
mod privileged;

fn socket_available() -> bool {
    let dir = ::test_support::private_tempdir().unwrap();
    match std::os::unix::net::UnixListener::bind(dir.path().join("probe.sock")) {
        Ok(_) => true,
        Err(e) if e.raw_os_error() == Some(1) => {
            eprintln!("pending unrestricted host: Unix socket bind denied by sandbox (EPERM)");
            false
        }
        Err(e) => panic!("socket probe failed: {e}"),
    }
}
#[path = "w6a3/operator.rs"]
mod operator;

#[test]
fn injected_nonroot_peer_is_forbidden_over_real_socket() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((1000, 42))),
    )
    .unwrap();
    let stream = UnixStream::connect(&c.general.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    // Refused before even a malformed frame is supplied.
    let mut line = String::new();
    assert_eq!(stream.read_line(&mut line).unwrap(), 0);
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}

#[test]
fn lifecycle_start_uses_kernel_credentials_over_real_socket() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let handle = api::start(Arc::new(c.clone()), context(&c), stop.clone()).unwrap();
    let stream = UnixStream::connect(&c.general.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    if rustix::process::geteuid().as_raw() == 0 {
        write(&mut stream, &request("status", "{}"));
        let response = read(&mut stream);
        assert!(response.ok, "{response:?}");
    } else {
        let mut line = String::new();
        assert_eq!(stream.read_line(&mut line).unwrap(), 0);
    }
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}

#[test]
fn socket_cleanup_uses_pinned_parent_and_preserves_replacement_leaf() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let parent = dir.path().join("runtime");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut c = socket_config(&dir);
    c.general.socket = parent.join("api.sock");
    let stop = Arc::new(AtomicBool::new(false));
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
    )
    .unwrap();
    let retained = dir.path().join("retained");
    std::fs::rename(&parent, &retained).unwrap();
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(c.general.socket.clone(), b"replacement parent sentinel").unwrap();
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    assert!(!retained.join("api.sock").exists());
    assert_eq!(
        std::fs::read(&c.general.socket).unwrap(),
        b"replacement parent sentinel"
    );
    std::fs::remove_file(&c.general.socket).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
    )
    .unwrap();
    std::fs::remove_file(&c.general.socket).unwrap();
    let target = parent.join("target");
    std::fs::write(&target, b"untouched").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink(&target, &c.general.socket).unwrap();
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    assert!(std::fs::symlink_metadata(&c.general.socket)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn full_backlog_probe_expires_and_concurrent_start_preserves_socket() {
    if !socket_available() {
        return;
    }
    use rustix::net::{self, AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
    let dir = ::test_support::private_tempdir().unwrap();
    let c = socket_config(&dir);
    let listener = net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    let addr = SocketAddrUnix::new(&c.general.socket).unwrap();
    net::bind(&listener, &addr).unwrap();
    net::listen(&listener, 1).unwrap();
    let mut clients = Vec::new();
    let mut full = false;
    for _ in 0..8 {
        let client = net::socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::NONBLOCK | SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        match net::connect(&client, &addr) {
            Ok(()) => clients.push(client),
            Err(rustix::io::Errno::AGAIN) => {
                full = true;
                break;
            }
            Err(e) => panic!("backlog setup: {e}"),
        }
    }
    assert!(full, "bounded backlog did not fill");
    let began = std::time::Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let result = api::start_with_peer_timeout(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
        Duration::from_millis(100),
    );
    assert!(result.is_err());
    assert!(began.elapsed() < Duration::from_secs(1));
    assert!(c.general.socket.exists());
    drop(clients);
    drop(listener);
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
    )
    .unwrap();
    let before = std::fs::symlink_metadata(&c.general.socket).unwrap();
    assert!(api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((0, 1)))
    )
    .is_err());
    use std::os::unix::fs::MetadataExt;
    let after = std::fs::symlink_metadata(&c.general.socket).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}

#[test]
fn api_and_metrics_share_a_private_directory() {
    if !socket_available() {
        return;
    }
    let dir = ::test_support::private_tempdir().unwrap();
    let mut c = socket_config(&dir);
    c.general.state_dir = dir.path().join("state");
    ::test_support::private_dir(&c.general.state_dir).unwrap();
    tidemark_store::catalog::Catalog::open(&c.general.state_dir, true).unwrap();
    c.roots.clear();
    c.metrics.textfile = dir.path().join("metrics.prom");
    c.metrics.interval = Duration::from_millis(50);
    let stop = Arc::new(AtomicBool::new(false));
    let ctx = context(&c);
    let state = ctx.state.clone();
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        ctx,
        stop.clone(),
        Arc::new(|_| Ok((0, 1))),
    )
    .unwrap();
    let exporter =
        tidemarkd::metrics::start(Arc::new(c.clone()), state.clone(), stop.clone()).unwrap();
    let lock = dir.path().join("api.sock.lock");
    assert_eq!(
        std::fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !state
        .view()
        .status(0)
        .unwrap()
        .components
        .iter()
        .any(|c| c.name == "metrics" && c.state == "ok")
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let exported = std::fs::read_to_string(&c.metrics.textfile);
    let mut stream = BufReader::new(UnixStream::connect(&c.general.socket).unwrap());
    stream
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write(&mut stream, &request("status", "{}"));
    let response = read(&mut stream);
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    exporter.join().unwrap().unwrap();
    assert!(response.ok);
    let status: StatusResult = serde_json::from_value(response.result.unwrap()).unwrap();
    assert!(status
        .components
        .iter()
        .any(|c| c.name == "metrics" && c.state == "ok"));
    assert!(exported.unwrap().contains("tidemark_up 1"));
    assert!(!lock.exists());
    assert!(!c.general.socket.exists());
}

#[test]
fn space_refusal_reply_and_status_are_published_before_acknowledgement() {
    use tidemark_core::{
        config::{RootConfig, RootMode},
        path::RawPath,
    };
    use tidemarkd::{scheduler, services::tswriter};
    let root = ::test_support::private_tempdir().unwrap();
    let storage = ::test_support::private_tempdir().unwrap();
    let mut c = Config::default();
    c.general.state_dir = storage.path().to_owned();
    c.retention.min_free_pct = 100.0;
    c.roots = vec![RootConfig {
        path: root.path().to_owned(),
        schedule: "off".into(),
        mode: RootMode::Walk,
        ..RootConfig::default()
    }];
    let state = LiveState::new(&c, 10);
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = Arc::new(AtomicBool::new(false));
    let (writer, wh) =
        tswriter::start(Arc::new(c.clone()), state.clone(), writer_stop.clone()).unwrap();
    let (control, sh) = scheduler::start(Arc::new(c), state.clone(), writer, stop.clone()).unwrap();
    let ctx = Context {
        state: state.clone(),
        scans: control,
        reload: Arc::new(|| unreachable!("reload unused")),
    };
    let r = Request {
        v: 1,
        id: "refused".into(),
        method: "scan.start".into(),
        params: serde_json::to_value(ScanStartParams {
            root: RawPath::from_path(root.path()).unwrap(),
            mode: Some(RootMode::Walk),
            throttle: None,
        })
        .unwrap(),
    };
    let response = api::dispatch(&r, 0, &ctx, 20).unwrap();
    let view = state.view();
    let status = api::dispatch(&request("status", "{}"), 0, &ctx, 20).unwrap();
    stop.store(true, Ordering::Release);
    sh.join().unwrap().unwrap();
    writer_stop.store(true, Ordering::Release);
    wh.join().unwrap().unwrap();
    let error = response.error.unwrap();
    assert_eq!(error.code, ApiErrorCode::Unavailable);
    assert_eq!(error.message, "scan: NoSpace");
    assert_eq!(view.state_space_refused(), Some(true));
    let report: StatusResult = serde_json::from_value(status.result.unwrap()).unwrap();
    let component = report
        .components
        .iter()
        .find(|c| c.name == "state_space")
        .unwrap();
    assert_eq!(component.state, "degraded");
    assert_eq!(component.detail, "scan: NoSpace");
}
