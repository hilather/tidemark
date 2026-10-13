//! Owned by: W6-A3
//! Bounded authenticated Unix JSON-lines API.
use crate::DaemonError;
use std::{
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::api::*;
const LIMIT: usize = 1024 * 1024;
const CONNECTIONS: usize = 32;
const TIMEOUT: Duration = Duration::from_secs(10);
/// Cooperative request cancellation and absolute handler deadline.
/// Cancellation cannot interrupt a kernel/SQLite call already in progress (R3-7).
#[derive(Clone)]
pub struct Cancellation {
    phase: Arc<AtomicUsize>,
    shutdown: crate::Shutdown,
    deadline: Instant,
}
impl Cancellation {
    /// Create an execution budget that also observes lifecycle shutdown.
    pub fn new(shutdown: crate::Shutdown, timeout: Duration) -> Self {
        Self {
            phase: Arc::new(AtomicUsize::new(0)),
            shutdown,
            deadline: Instant::now() + timeout,
        }
    }
    /// Reject uncommitted work when cancellation, shutdown or the deadline wins.
    pub fn check(&self) -> Result<(), DaemonError> {
        if self.phase.load(Ordering::Acquire) == 1
            || self.shutdown.load(Ordering::Acquire)
            || Instant::now() >= self.deadline
        {
            Err(DaemonError::Shutdown)
        } else {
            Ok(())
        }
    }
    fn cancel(&self) {
        let _ = self
            .phase
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }
    /// Claim the configuration commit boundary atomically against cancellation.
    /// Once claimed, publication completes even if a transport timeout follows;
    /// clients must treat a timeout during commit as an uncertain outcome.
    pub fn begin_commit(&self) -> Result<(), DaemonError> {
        self.check()?;
        self.phase
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| DaemonError::Shutdown)
    }
}
/// Internal reload execution contract. Production implementations observe the budget.
pub trait ReloadCallback: Send + Sync {
    /// Execute a reload with cooperative cancellation and an absolute deadline.
    fn call(&self, cancellation: &Cancellation) -> Result<ConfigReloadResult, DaemonError>;
}
// Retain compatibility for existing injected zero-argument test callbacks.
// Such callbacks are owned/joined but cannot promise cooperative cancellation.
impl<F> ReloadCallback for F
where
    F: Fn() -> Result<ConfigReloadResult, DaemonError> + Send + Sync,
{
    fn call(&self, cancellation: &Cancellation) -> Result<ConfigReloadResult, DaemonError> {
        cancellation.check()?;
        self()
    }
}
/// Validating lifecycle reload callback, retaining the old config on failure.
pub type Reload = Arc<dyn ReloadCallback>;
/// Adapt a callback that explicitly consumes the request execution budget.
pub struct CooperativeReload<F>(pub F);
impl<F> ReloadCallback for CooperativeReload<F>
where
    F: Fn(&Cancellation) -> Result<ConfigReloadResult, DaemonError> + Send + Sync,
{
    fn call(&self, cancellation: &Cancellation) -> Result<ConfigReloadResult, DaemonError> {
        (self.0)(cancellation)
    }
}
/// Shared handler dependencies; measurements remain in LiveState.
pub struct Context {
    /// Measurement publication handle.
    pub state: crate::state::LiveState,
    /// Single-scan scheduler control.
    pub scans: crate::scheduler::ScanControl,
    /// Lifecycle reload callback.
    pub reload: Reload,
}
fn error(id: &str, code: ApiErrorCode, message: impl Into<String>) -> Response {
    Response {
        v: 1,
        id: id.into(),
        ok: false,
        result: None,
        error: Some(ApiError {
            code,
            message: message.into(),
        }),
    }
}
fn params<T: serde::de::DeserializeOwned>(r: &Request) -> Result<T, DaemonError> {
    serde_json::from_value(r.params.clone()).map_err(|e| DaemonError::Invalid(e.to_string()))
}
fn value<T: serde::Serialize>(v: T) -> Result<serde_json::Value, DaemonError> {
    serde_json::to_value(v).map_err(|e| DaemonError::Internal(e.to_string()))
}
fn available<T>(v: Option<T>) -> Result<T, DaemonError> {
    v.ok_or_else(|| DaemonError::Unavailable("measurement unavailable".into()))
}
fn code(e: &DaemonError) -> ApiErrorCode {
    match e {
        DaemonError::Invalid(_) | DaemonError::Config(_) => ApiErrorCode::BadRequest,
        DaemonError::Unavailable(_) | DaemonError::Shutdown | DaemonError::NotYetImplemented => {
            ApiErrorCode::Unavailable
        }
        DaemonError::Scan(s) if s == "Busy" => ApiErrorCode::Busy,
        DaemonError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            ApiErrorCode::NotFound
        }
        _ => ApiErrorCode::Internal,
    }
}
/// Dispatch all frozen methods after peer authorization. `alerts.history`
/// contains only successfully persisted events from the current daemon run.
pub fn dispatch(
    request: &Request,
    peer_uid: u32,
    context: &Context,
    now: i64,
) -> Result<Response, DaemonError> {
    dispatch_controlled(
        request,
        peer_uid,
        context,
        now,
        &Cancellation::new(Arc::new(std::sync::atomic::AtomicBool::new(false)), TIMEOUT),
    )
}
fn dispatch_controlled(
    request: &Request,
    peer_uid: u32,
    context: &Context,
    now: i64,
    cancellation: &Cancellation,
) -> Result<Response, DaemonError> {
    if peer_uid != 0 {
        return Ok(error(
            &request.id,
            ApiErrorCode::Forbidden,
            "peer uid forbidden",
        ));
    }
    if request.v != 1 {
        return Ok(error(
            &request.id,
            ApiErrorCode::BadRequest,
            "unsupported API version",
        ));
    }
    let result = (|| {
        cancellation.check()?;
        let view = context.state.view_at(now);
        match request.method.as_str() {
            "status" => {
                let _: StatusParams = params(request)?;
                value(available(view.status(now))?)
            }
            "now" => value(available(view.now(&params(request)?, now))?),
            "hot" => value(available(view.hot(&params(request)?))?),
            "lvm" => value(available(view.lvm_report(&params(request)?))?),
            "phantom" => value(available(view.phantom(&params(request)?))?),
            "writers" => value(available(view.writers(&params(request)?))?),
            "alerts.active" => {
                let p: AlertsActiveParams = params(request)?;
                let mut rows = available(view.active_alerts())?;
                rows.truncate(p.limit.unwrap_or(rows.len()));
                value(rows)
            }
            "alerts.history" => value(view.alerts_history(&params(request)?)?),
            "scan.start" => value(context.scans.start(&params(request)?)?),
            "scan.status" => value(context.scans.status(&params(request)?)?),
            "scan.cancel" => value(context.scans.cancel(&params(request)?)?),
            "config.reload" => {
                let _: ConfigReloadParams = params(request)?;
                value(context.reload.call(cancellation)?)
            }
            _ => Err(DaemonError::Invalid("unknown method".into())),
        }
    })();
    Ok(match result {
        Ok(result) => Response {
            v: 1,
            id: request.id.clone(),
            ok: true,
            result: Some(result),
            error: None,
        },
        Err(e) => error(&request.id, code(&e), e.to_string()),
    })
}
struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
// A connection owns its handler even after a transport timeout. Joining here
// also makes service shutdown wait for callbacks and retain the admission slot.
// R3-7: neither a deadline nor shutdown can interrupt kernel I/O in progress.
struct Handler(Option<std::thread::JoinHandle<()>>);
impl Drop for Handler {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.join();
        }
    }
}
fn io(e: std::io::Error) -> DaemonError {
    DaemonError::Io {
        context: "API socket".into(),
        source: e,
    }
}
struct Frame(Vec<u8>);
impl Write for Frame {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(b.len()) >= LIMIT {
            return Err(std::io::Error::other("frame exceeds 1 MiB"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn send(
    stream: &mut UnixStream,
    response: &Response,
    shutdown: &crate::Shutdown,
    timeout: Duration,
) -> std::io::Result<()> {
    let mut bytes = Frame(Vec::with_capacity(8192));
    if serde_json::to_writer(&mut bytes, response).is_err() {
        bytes.0.clear();
        serde_json::to_writer(
            &mut bytes,
            &error(
                &response.id,
                ApiErrorCode::Unavailable,
                "response exceeds 1 MiB",
            ),
        )
        .map_err(std::io::Error::other)?;
    }
    bytes.0.push(b'\n');
    let deadline = Instant::now() + timeout;
    let mut offset = 0;
    while offset < bytes.0.len() {
        if shutdown.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        stream.set_write_timeout(Some(remaining.min(Duration::from_millis(50))))?;
        match stream.write(&bytes.0[offset..]) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => offset += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn connection(
    mut stream: UnixStream,
    context: Arc<Context>,
    shutdown: crate::Shutdown,
    permit: Permit,
    peer: PeerCredentials,
    timeout: Duration,
    accepted: Instant,
) {
    let (uid, pid) = match peer(&stream) {
        Ok(c) => c,
        Err(_) => return,
    };
    if uid != 0 {
        return;
    }
    let mut read_deadline = accepted + timeout;
    if stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .is_err()
    {
        return;
    }
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    let mut line = Vec::with_capacity(8192);
    let mut buffer = [0; 8192];
    let permit = Arc::new(permit);
    while !shutdown.load(Ordering::Acquire) && Instant::now() < read_deadline {
        let n = match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => return,
        };
        for &b in &buffer[..n] {
            if shutdown.load(Ordering::Acquire) || Instant::now() >= read_deadline {
                return;
            }
            if line.len() + 1 > LIMIT {
                let _ = send(
                    &mut stream,
                    &error("", ApiErrorCode::BadRequest, "request line exceeds 1 MiB"),
                    &shutdown,
                    timeout,
                );
                return;
            }
            if b != b'\n' {
                line.push(b);
                continue;
            }
            let request: Request = match serde_json::from_slice(&line) {
                Ok(r) => r,
                Err(e) => {
                    line.clear();
                    if send(
                        &mut stream,
                        &error("", ApiErrorCode::BadRequest, e.to_string()),
                        &shutdown,
                        timeout,
                    )
                    .is_err()
                    {
                        return;
                    }
                    read_deadline = Instant::now() + timeout;
                    continue;
                }
            };
            line.clear();
            tracing::info!(uid, pid, method = %request.method, "API request");
            let id = request.id.clone();
            let ctx = context.clone();
            let retained = permit.clone();
            let (tx, rx) = crossbeam_channel::bounded(1);
            let cancellation = Cancellation::new(shutdown.clone(), timeout);
            let budget = cancellation.clone();
            // A timed-out handler retains its admission permit until it exits,
            // bounding even callbacks that cannot be cancelled by their owner.
            let spawned = std::thread::Builder::new()
                .name("api-handler".into())
                .spawn(move || {
                    let _retained = retained;
                    let _ = tx.send(dispatch_controlled(
                        &request,
                        uid,
                        &ctx,
                        crate::unix_now(),
                        &budget,
                    ));
                });
            let _handler = match spawned {
                Ok(handle) => Handler(Some(handle)),
                Err(_) => return,
            };
            let deadline = cancellation.deadline;
            let mut timed_out = false;
            let response = loop {
                if shutdown.load(Ordering::Acquire) {
                    cancellation.cancel();
                    return;
                }
                if Instant::now() >= deadline {
                    cancellation.cancel();
                    timed_out = true;
                    break error(&id, ApiErrorCode::Unavailable, "handler timed out");
                }
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(Ok(r)) => break r,
                    Ok(Err(e)) => break error(&id, code(&e), e.to_string()),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break error(&id, ApiErrorCode::Internal, "handler disconnected"),
                }
            };
            if send(&mut stream, &response, &shutdown, timeout).is_err() {
                return;
            }
            // Do not admit more handlers on a connection with a stuck callback.
            if timed_out {
                return;
            }
            read_deadline = Instant::now() + timeout;
        }
    }
}
/// Start the lifecycle transport with kernel-verified, root-only peer credentials.
pub fn start(
    config: Arc<tidemark_core::config::Config>,
    context: Context,
    shutdown: crate::Shutdown,
) -> Result<crate::ServiceHandle, DaemonError> {
    start_with_peer(
        config,
        context,
        shutdown,
        Arc::new(|stream| {
            rustix::net::sockopt::socket_peercred(stream)
                .map(|c| (c.uid.as_raw(), c.pid.as_raw_pid()))
                .map_err(std::io::Error::from)
        }),
    )
}
/// Credential provider returning verified (uid, pid), injectable for transport tests.
pub type PeerCredentials = Arc<dyn Fn(&UnixStream) -> std::io::Result<(u32, i32)> + Send + Sync>;
/// Start transport with a credential provider. Production must supply
/// SO_PEERCRED; callers must never infer credentials from socket ownership.
pub fn start_with_peer(
    config: Arc<tidemark_core::config::Config>,
    context: Context,
    shutdown: crate::Shutdown,
    peer: PeerCredentials,
) -> Result<crate::ServiceHandle, DaemonError> {
    start_with_peer_timeout(config, context, shutdown, peer, TIMEOUT)
}
/// Controllable deadline seam for tests. Production handlers use ten seconds.
pub fn start_with_peer_timeout(
    config: Arc<tidemark_core::config::Config>,
    context: Context,
    shutdown: crate::Shutdown,
    peer: PeerCredentials,
    timeout: Duration,
) -> Result<crate::ServiceHandle, DaemonError> {
    let path = config.general.socket.clone();
    match std::fs::symlink_metadata(&path) {
        Ok(m) => {
            if !m.file_type().is_socket() {
                return Err(DaemonError::Invalid("API path is not a socket".into()));
            }
            match UnixStream::connect(&path) {
                Ok(_) => return Err(DaemonError::Unavailable("API socket already active".into())),
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    std::fs::remove_file(&path).map_err(io)?
                }
                Err(e) => return Err(io(e)),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(e)),
    }
    let listener = UnixListener::bind(&path).map_err(io)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
    listener.set_nonblocking(true).map_err(io)?;
    context
        .state
        .publish_component(crate::component("api", "ok", "Unix socket listening"))?;
    let context = Arc::new(context);
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("api".into())
        .spawn(move || {
            let mut clients: Vec<std::thread::JoinHandle<()>> = Vec::new();
            let mut outcome = Ok(());
            while !shutdown.load(Ordering::Acquire) {
                let mut index = 0;
                while index < clients.len() {
                    if clients[index].is_finished() {
                        let _ = clients.swap_remove(index).join();
                    } else {
                        index += 1;
                    }
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let accepted = Instant::now();
                        if clients.len() >= CONNECTIONS
                            || active.load(Ordering::Acquire) >= CONNECTIONS
                        {
                            drop(stream);
                            continue;
                        }
                        active.fetch_add(1, Ordering::AcqRel);
                        let permit = Permit(active.clone());
                        let ctx = context.clone();
                        let stop = shutdown.clone();
                        let peer = peer.clone();
                        if let Ok(h) =
                            std::thread::Builder::new()
                                .name("api-client".into())
                                .spawn(move || {
                                    connection(stream, ctx, stop, permit, peer, timeout, accepted)
                                })
                        {
                            clients.push(h);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::park_timeout(Duration::from_millis(20))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        outcome = Err(io(e));
                        shutdown.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            drop(listener);
            let cleanup = std::fs::remove_file(&path).map_err(io);
            for h in clients {
                let _ = h.join();
            }
            outcome.and(cleanup)
        })
        .map_err(io)
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::{BufRead, BufReader};
    fn pair(
        reload: Reload,
        timeout: Duration,
    ) -> (
        BufReader<UnixStream>,
        crate::Shutdown,
        std::thread::JoinHandle<()>,
    ) {
        let (server, client) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ctx = Arc::new(Context {
            state: crate::state::LiveState::new(&tidemark_core::config::Config::default(), 0),
            scans: crate::scheduler::ScanControl::default(),
            reload,
        });
        let flag = stop.clone();
        let h = std::thread::spawn(move || {
            connection(
                server,
                ctx,
                flag,
                Permit(Arc::new(AtomicUsize::new(1))),
                Arc::new(|_| Ok((0, 1))),
                timeout,
                Instant::now(),
            )
        });
        (BufReader::new(client), stop, h)
    }
    fn response(s: &mut BufReader<UnixStream>) -> Response {
        let mut line = String::new();
        s.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    #[test]
    fn pair_framing_partial_malformed_oversize_and_idle_stop() {
        let reload: Reload = Arc::new(|| {
            Ok(ConfigReloadResult {
                reloaded: true,
                warnings: vec![],
            })
        });
        let (mut s, stop, h) = pair(reload, TIMEOUT);
        s.get_mut().write_all(b"bad\n").unwrap();
        assert_eq!(
            response(&mut s).error.unwrap().code,
            ApiErrorCode::BadRequest
        );
        s.get_mut().write_all(b"{\"v\":1,\"id\":\"x\",").unwrap();
        s.get_mut()
            .write_all(b"\"method\":\"status\",\"params\":{}}\n")
            .unwrap();
        assert!(response(&mut s).ok);
        s.get_mut()
            .write_all(b"{\"v\":1,\"id\":\"y\",\"method\":\"config.reload\",\"params\":{}}\n")
            .unwrap();
        assert!(response(&mut s).ok);
        s.get_mut().write_all(&vec![b'x'; LIMIT]).unwrap();
        s.get_mut().write_all(b"\n").unwrap();
        assert_eq!(
            response(&mut s).error.unwrap().code,
            ApiErrorCode::BadRequest
        );
        stop.store(true, Ordering::Release);
        h.join().unwrap();
    }
    #[test]
    fn idle_and_drip_frames_have_absolute_deadlines() {
        for drip in [false, true] {
            let reload: Reload = Arc::new(|| unreachable!("incomplete frame dispatched"));
            let (mut stream, _stop, handle) = pair(reload, Duration::from_millis(120));
            let began = Instant::now();
            if drip {
                for _ in 0..4 {
                    if stream.get_mut().write_all(b"x").is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(45));
                }
            }
            let mut byte = [0];
            assert_eq!(stream.get_mut().read(&mut byte).unwrap(), 0);
            handle.join().unwrap();
            assert!(began.elapsed() < Duration::from_secs(1));
        }
    }
    #[test]
    fn response_write_deadline_and_shutdown_interrupt_stalled_reader() {
        for stopping in [false, true] {
            let (mut server, _client) = UnixStream::pair().unwrap();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(stopping));
            let response = Response {
                v: 1,
                id: "x".into(),
                ok: true,
                result: Some(serde_json::json!("x".repeat(900_000))),
                error: None,
            };
            let began = Instant::now();
            assert!(send(&mut server, &response, &stop, Duration::from_millis(120)).is_err());
            assert!(began.elapsed() < Duration::from_secs(1));
        }
    }
    #[test]
    fn peer_refusal_precedes_malformed_frame_and_credential_failure_closes() {
        for peer in [
            Arc::new(|_: &UnixStream| Ok((1000u32, 42))) as PeerCredentials,
            Arc::new(|_: &UnixStream| Err(std::io::ErrorKind::PermissionDenied.into())),
        ] {
            let (server, mut client) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            client.write_all(b"{malformed}\n").unwrap();
            let context = Arc::new(Context {
                state: crate::state::LiveState::new(&tidemark_core::config::Config::default(), 0),
                scans: crate::scheduler::ScanControl::default(),
                reload: Arc::new(|| unreachable!("unauthorized dispatch")),
            });
            let active = Arc::new(AtomicUsize::new(1));
            connection(
                server,
                context,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                Permit(active.clone()),
                peer,
                TIMEOUT,
                Instant::now(),
            );
            let mut byte = [0];
            match client.read(&mut byte) {
                Ok(n) => assert_eq!(n, 0, "unauthorized peer received response bytes"),
                Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset),
            }
            assert_eq!(active.load(Ordering::Acquire), 0);
        }
    }
    #[test]
    fn cooperative_reload_exits_on_deadline_and_shutdown() {
        for stopping in [false, true] {
            let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
            let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = exited.clone();
            let reload: Reload = Arc::new(CooperativeReload(move |budget: &Cancellation| {
                entered_tx.send(()).unwrap();
                while budget.check().is_ok() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                flag.store(true, Ordering::Release);
                Err(DaemonError::Shutdown)
            }));
            let (mut stream, stop, handle) = pair(reload, Duration::from_millis(100));
            stream
                .get_mut()
                .write_all(b"{\"v\":1,\"id\":\"x\",\"method\":\"config.reload\",\"params\":{}}\n")
                .unwrap();
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            if stopping {
                stop.store(true, Ordering::Release);
            } else {
                assert_eq!(
                    response(&mut stream).error.unwrap().code,
                    ApiErrorCode::Unavailable
                );
            }
            handle.join().unwrap();
            assert!(exited.load(Ordering::Acquire));
        }
    }
    #[test]
    fn cancellation_and_commit_have_an_atomic_winner() {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled = Cancellation::new(stop.clone(), TIMEOUT);
        cancelled.cancel();
        assert!(cancelled.begin_commit().is_err());
        let committed = Cancellation::new(stop.clone(), TIMEOUT);
        committed.begin_commit().unwrap();
        committed.cancel();
        assert_eq!(committed.phase.load(Ordering::Acquire), 2);
        let expired = Cancellation::new(stop.clone(), Duration::ZERO);
        assert!(expired.begin_commit().is_err());
        let stopping = Cancellation::new(stop.clone(), TIMEOUT);
        stop.store(true, Ordering::Release);
        assert!(stopping.begin_commit().is_err());
    }
    #[test]
    fn pair_controllable_timeout_and_shutdown() {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let reload: Reload = Arc::new(move || {
            rx.recv().unwrap();
            Ok(ConfigReloadResult {
                reloaded: true,
                warnings: vec![],
            })
        });
        let (mut s, stop, h) = pair(reload, Duration::from_millis(50));
        s.get_mut()
            .write_all(b"{\"v\":1,\"id\":\"x\",\"method\":\"config.reload\",\"params\":{}}\n")
            .unwrap();
        assert_eq!(
            response(&mut s).error.unwrap().code,
            ApiErrorCode::Unavailable
        );
        stop.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(80));
        assert!(!h.is_finished(), "server abandoned its blocked callback");
        tx.send(()).unwrap();
        h.join().unwrap();
    }
}
