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
    let own = rustix::process::geteuid().as_raw();
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
        for uid in [0, own] {
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
    let dir = tempfile::tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(|_| Ok((rustix::process::geteuid().as_raw(), 1))),
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
    let dir = tempfile::tempdir().unwrap();
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
    handle.join().unwrap().unwrap();
    release_tx.send(()).unwrap();
}
#[test]
fn refuses_non_socket_and_active_socket_removes_stale_socket() {
    if !socket_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let c = socket_config(&dir);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = crossbeam_channel::bounded(32);
    let handle = api::start_with_peer(
        Arc::new(c.clone()),
        context(&c),
        stop.clone(),
        Arc::new(move |_| {
            tx.send(()).unwrap();
            Ok((0, 1))
        }),
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
    stop.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
}
#[cfg(feature = "privileged")]
#[path = "w6a3/privileged.rs"]
mod privileged;

fn socket_available() -> bool {
    let dir = tempfile::tempdir().unwrap();
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
