//! Owned by: W6-A3
//! Bounded authenticated Unix JSON-lines API.
use crate::DaemonError;
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tidemark_core::api::*;
const LIMIT: usize = 1024 * 1024;
/// Pure incremental JSON-line framing state, bounded to one MiB including newline.
/// Reuse after Incomplete to supply more bytes, or after InvalidJson for the next
/// line. No transport, thread, peer check or clock is stored here.
#[doc(hidden)]
pub struct FrameDecoder {
    line: Vec<u8>,
}
impl Default for FrameDecoder {
    fn default() -> Self {
        Self {
            line: Vec::with_capacity(8192),
        }
    }
}
/// Typed refusals from the pure request framing seam.
#[derive(Debug, thiserror::Error)]
#[doc(hidden)]
pub enum FrameError {
    /// Input ended with no pending frame.
    #[error("end of input")]
    EndOfInput,
    /// EOF before newline; decoder retains the bounded partial frame.
    #[error("incomplete request line")]
    Incomplete,
    /// Limit counts the terminating newline, matching the connection handler.
    #[error("request line exceeds 1 MiB")]
    TooLarge,
    /// A complete line did not decode as a Request; decoder is reset.
    #[error("{0}")]
    InvalidJson(serde_json::Error),
    /// Read failure; the caller decides whether to retry or close.
    #[error("{0}")]
    Io(std::io::Error),
}
/// Read one JSON-line request, leaving subsequent bytes unread.
/// Work is linear in bytes read, with at most LIMIT bytes of framing capacity.
/// EOF returns EndOfInput/Incomplete. Read errors return immediately (including
/// Interrupted), so an adversarial reader cannot trigger a retry loop here.
/// The connection owns retries, timeouts, credentials and dispatch.
#[doc(hidden)]
pub fn read_frame(
    reader: &mut impl Read,
    decoder: &mut FrameDecoder,
) -> Result<Request, FrameError> {
    let mut byte = [0];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => {
                return Err(if decoder.line.is_empty() {
                    FrameError::EndOfInput
                } else {
                    FrameError::Incomplete
                })
            }
            Ok(_) => {}
            Err(error) => return Err(FrameError::Io(error)),
        }
        if decoder.line.len() + 1 > LIMIT {
            return Err(FrameError::TooLarge);
        }
        if byte[0] != b'\n' {
            decoder.line.push(byte[0]);
            continue;
        }
        let result = serde_json::from_slice(&decoder.line).map_err(FrameError::InvalidJson);
        decoder.line.clear();
        return result;
    }
}

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
        // Configuration commits (phase 2) remain irrevocable. Scan admission
        // (phase 3) stays cancellable while its acknowledgement is outstanding.
        let _ = self
            .phase
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |phase| {
                matches!(phase, 0 | 3).then_some(1)
            });
    }
    /// Claim scan admission against cancellation while keeping the request cancellable.
    pub(crate) fn begin_scan_admission(&self) -> Result<(), DaemonError> {
        self.check()?;
        self.phase
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| DaemonError::Shutdown)
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
pub(crate) fn log_request(uid: u32, pid: i32, method: &str, ok: bool, elapsed: Duration) {
    let outcome = if ok { "ok" } else { "failed" };
    let millis = elapsed.as_millis();
    tracing::info!(uid, pid, method = %method, ok, millis = %millis, "API request: {method} from uid {uid} pid {pid}, {outcome} in {millis} ms");
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
        DaemonError::Scan(s) if s == "NoSpace" => ApiErrorCode::Unavailable,
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
/// Dispatch an authenticated request with its caller-supplied execution budget.
pub(crate) fn dispatch_controlled(
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
            "hot" => {
                let p = params(request)?;
                value(
                    view.hot(&p)
                        .ok_or_else(|| DaemonError::Unavailable(view.hot_unavailable_detail(&p)))?,
                )
            }
            "lvm" => value(available(view.lvm_report(&params(request)?))?),
            "phantom" => value(available(view.phantom(&params(request)?))?),
            "writers" => value(
                view.writers(&params(request)?)
                    .ok_or_else(|| DaemonError::Unavailable(view.process_unavailable_detail()))?,
            ),
            "alerts.active" => {
                let p: AlertsActiveParams = params(request)?;
                let mut rows = available(view.active_alerts())?;
                rows.truncate(p.limit.unwrap_or(rows.len()));
                value(rows)
            }
            "alerts.history" => value(view.alerts_history(&params(request)?)?),
            "scan.start" => value(
                context
                    .scans
                    .start_for_request(&params(request)?, cancellation)?,
            ),
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
fn socket_lock_error(error: std::io::Error, path: &std::path::Path) -> std::io::Error {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        std::io::Error::new(error.kind(), format!(
            "another tidemarkd is already running with this configuration (API socket lock {} is held)",
            tidemark_core::path::display_lossy_bytes(path.as_os_str().as_encoded_bytes())
        ))
    } else {
        error
    }
}

#[cfg(test)]
mod lock_error_tests {
    #[test]
    fn held_socket_lock_explains_refusal_without_binding() {
        let path = std::path::Path::new("/run/tidemark/api.sock.lock");
        let error = super::socket_lock_error(std::io::ErrorKind::WouldBlock.into(), path);
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(super::io(error).to_string(), "API socket: another tidemarkd is already running with this configuration (API socket lock /run/tidemark/api.sock.lock is held)");
        let other = super::socket_lock_error(std::io::ErrorKind::PermissionDenied.into(), path);
        assert_eq!(other.kind(), std::io::ErrorKind::PermissionDenied);
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
    let mut frame = FrameDecoder::default();
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
            // Keep the existing per-byte shutdown/deadline boundary above.
            // The pure seam retains incomplete bytes across one-byte slices.
            let request = match read_frame(&mut std::slice::from_ref(&b), &mut frame) {
                Ok(request) => request,
                Err(FrameError::Incomplete) => continue,
                Err(FrameError::TooLarge) => {
                    let _ = send(
                        &mut stream,
                        &error("", ApiErrorCode::BadRequest, "request line exceeds 1 MiB"),
                        &shutdown,
                        timeout,
                    );
                    return;
                }
                Err(FrameError::InvalidJson(e)) => {
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
                Err(_) => return,
            };
            let request_started = Instant::now();
            let method = request.method.clone();
            let scan_start = request.method == "scan.start";
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
                Err(_) => {
                    log_request(uid, pid, &method, false, request_started.elapsed());
                    return;
                }
            };
            let deadline = cancellation.deadline;
            let mut timed_out = false;
            let response = loop {
                if shutdown.load(Ordering::Acquire) {
                    cancellation.cancel();
                    log_request(uid, pid, &method, false, request_started.elapsed());
                    return;
                }
                if Instant::now() >= deadline {
                    cancellation.cancel();
                    timed_out = true;
                    break error(
                        &id,
                        ApiErrorCode::Unavailable,
                        if scan_start {
                            "scan start handler timed out; outcome uncertain"
                        } else {
                            "handler timed out"
                        },
                    );
                }
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(Ok(r)) => break r,
                    Ok(Err(e)) => break error(&id, code(&e), e.to_string()),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break error(&id, ApiErrorCode::Internal, "handler disconnected"),
                }
            };
            log_request(uid, pid, &method, response.ok, request_started.elapsed());
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
// Unix bind/connect have no *at variant. Their pathname is derived exclusively
// from this retained directory fd, never from the mutable configured ancestry.
struct SocketPath {
    parent: std::os::fd::OwnedFd,
    name: std::ffi::OsString,
    identity: Option<(u64, u64)>,
    lock: std::os::fd::OwnedFd,
    lock_name: std::ffi::OsString,
    lock_identity: (u64, u64),
}
impl SocketPath {
    fn open(path: &std::path::Path) -> std::io::Result<Self> {
        use rustix::fs::{self, Mode, OFlags};
        use std::path::Component;
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("socket parent missing"))?;
        let name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("socket name missing"))?;
        if !path.is_absolute() {
            return Err(std::io::Error::other("socket path must be absolute"));
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut fd = fs::open("/", flags, Mode::empty())?;
        let uid = rustix::process::geteuid().as_raw();
        let mut sticky = false;
        for component in parent.components() {
            let stat = fs::fstat(&fd)?;
            if stat.st_uid != 0 && stat.st_uid != uid {
                use std::os::fd::AsRawFd;
                let ancestor = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))?;
                let ancestor = tidemark_core::path::display_lossy_bytes(
                    ancestor.as_os_str().as_encoded_bytes(),
                );
                let read_only = fs::fstatvfs(&fd)?
                    .f_flag
                    .contains(fs::StatVfsMountFlags::RDONLY);
                if tidemark_store::state_dir::strict_ancestry() || !read_only {
                    return Err(std::io::Error::other(format!("socket ancestor {ancestor} has untrusted owner {}; foreign owner on a read-only mount is not trusted", stat.st_uid)));
                }
                static WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    tracing::warn!("development build: accepted ancestor {} owned by {} (foreign owner on read-only mount); a release build refuses this", ancestor, stat.st_uid);
                }
            }
            if stat.st_mode & 0o022 != 0 {
                if stat.st_mode & 0o1000 == 0 {
                    return Err(std::io::Error::other(
                        "socket ancestor is writable by another user",
                    ));
                }
                sticky = true;
            }
            let name = match component {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                _ => return Err(std::io::Error::other("unsafe socket ancestor component")),
            };
            fd = match fs::openat(&fd, name, flags | OFlags::NOATIME, Mode::empty()) {
                Err(rustix::io::Errno::PERM) => fs::openat(&fd, name, flags, Mode::empty())?,
                result => result?,
            };
            if sticky {
                let child = fs::fstat(&fd)?;
                if child.st_uid != uid || child.st_mode & 0o777 != 0o700 {
                    return Err(std::io::Error::other(
                        "sticky ancestor requires a private owned descendant",
                    ));
                }
                sticky = false;
            }
        }
        let stat = fs::fstat(&fd)?;
        if (stat.st_uid != 0 && stat.st_uid != uid) || stat.st_mode & 0o022 != 0 {
            return Err(std::io::Error::other("socket parent is not trusted"));
        }
        // Serialize only this socket, retaining the parent for identity checks.
        let mut lock_name = name.to_owned();
        lock_name.push(".lock");
        let lock = fs::openat(
            &fd,
            &lock_name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let lock_stat = fs::fstat(&lock)?;
        if fs::FileType::from_raw_mode(lock_stat.st_mode) != fs::FileType::RegularFile
            || lock_stat.st_uid != uid
            || lock_stat.st_nlink != 1
        {
            return Err(std::io::Error::other(
                "API lock must be an owned regular single-link file",
            ));
        }
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|error| socket_lock_error(error.into(), &path.with_file_name(&lock_name)))?;
        let named = fs::statat(&fd, &lock_name, fs::AtFlags::SYMLINK_NOFOLLOW)?;
        if (named.st_dev, named.st_ino) != (lock_stat.st_dev, lock_stat.st_ino) {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        fs::fchmod(&lock, Mode::from_raw_mode(0o600))?;
        Ok(Self {
            parent: fd,
            name: name.to_owned(),
            identity: None,
            lock,
            lock_name,
            lock_identity: (lock_stat.st_dev, lock_stat.st_ino),
        })
    }
    fn address(&self) -> std::path::PathBuf {
        use std::os::fd::AsRawFd;
        std::path::PathBuf::from(format!("/proc/self/fd/{}/", self.parent.as_raw_fd()))
            .join(&self.name)
    }
    fn stat(&self) -> std::io::Result<rustix::fs::Stat> {
        rustix::fs::statat(
            &self.parent,
            &self.name,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(Into::into)
    }
    fn same(&self, identity: (u64, u64)) -> std::io::Result<bool> {
        match self.stat() {
            Ok(stat) => Ok((stat.st_dev, stat.st_ino) == identity
                && rustix::fs::FileType::from_raw_mode(stat.st_mode)
                    == rustix::fs::FileType::Socket),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn unlink(&self, identity: (u64, u64)) -> std::io::Result<()> {
        if self.same(identity)? {
            rustix::fs::unlinkat(&self.parent, &self.name, rustix::fs::AtFlags::empty())?;
        }
        Ok(())
    }
    fn prepare(&self, timeout: Duration) -> std::io::Result<()> {
        let stat = match self.stat() {
            Ok(stat) => stat,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Socket {
            return Err(std::io::Error::other("API path is not a socket"));
        }
        let identity = (stat.st_dev, stat.st_ino);
        use rustix::net::{self, AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
        let probe = net::socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::NONBLOCK | SocketFlags::CLOEXEC,
            None,
        )?;
        let addr = SocketAddrUnix::new(self.address())?;
        let deadline = Instant::now() + timeout;
        loop {
            match net::connect(&probe, &addr) {
                Ok(()) => return Err(std::io::Error::other("API socket already active")),
                Err(rustix::io::Errno::CONNREFUSED) => {
                    if !self.same(identity)? {
                        return Err(std::io::Error::other("API socket changed during probe"));
                    }
                    return self.unlink(identity);
                }
                Err(
                    rustix::io::Errno::AGAIN
                    | rustix::io::Errno::INPROGRESS
                    | rustix::io::Errno::ALREADY,
                ) => {
                    if Instant::now() >= deadline {
                        return Err(std::io::ErrorKind::TimedOut.into());
                    }
                    std::thread::park_timeout(Duration::from_millis(10));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn secure(&mut self) -> std::io::Result<()> {
        use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
        use std::os::fd::AsRawFd;
        // Linux rustix chmodat does not support SYMLINK_NOFOLLOW (including EL8).
        // Pin the socket inode with O_PATH|NOFOLLOW, reject substituted symlinks,
        // then chmod the pinned inode via its procfd. A replacement leaf cannot
        // redirect this chmod to its target. No untrusted pathname is followed.
        let leaf = fs::openat(
            &self.parent,
            &self.name,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let stat = fs::fstat(&leaf)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Socket
            || stat.st_uid != rustix::process::geteuid().as_raw()
        {
            return Err(std::io::Error::other("bound API socket replaced"));
        }
        let identity = (stat.st_dev, stat.st_ino);
        self.identity = Some(identity);
        if !self.same(identity)? {
            return Err(std::io::Error::other("bound API socket changed"));
        }
        fs::chmodat(
            &self.parent,
            format!("/proc/self/fd/{}", leaf.as_raw_fd()),
            Mode::from_raw_mode(0o600),
            AtFlags::empty(),
        )?;
        if !self.same(identity)? {
            return Err(std::io::Error::other("bound API socket changed"));
        }
        Ok(())
    }
}
impl Drop for SocketPath {
    fn drop(&mut self) {
        // Hold the lock through both identity-checked unlinks. A startup that
        // opened this inode before cleanup must reject its now-unlinked handle.
        let _keep_locked = &self.lock;
        if let Some(identity) = self.identity {
            if let Err(error) = self.unlink(identity) {
                tracing::warn!(%error, "API socket cleanup failed: {error}");
            }
        }
        if let Ok(stat) = rustix::fs::statat(
            &self.parent,
            &self.lock_name,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        ) {
            if (stat.st_dev, stat.st_ino) == self.lock_identity {
                if let Err(error) = rustix::fs::unlinkat(
                    &self.parent,
                    &self.lock_name,
                    rustix::fs::AtFlags::empty(),
                ) {
                    tracing::warn!(%error, "API lock cleanup failed: {error}");
                }
            }
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
    let mut path = SocketPath::open(&config.general.socket).map_err(io)?;
    path.prepare(timeout).map_err(io)?;
    let listener = UnixListener::bind(path.address()).map_err(io)?;
    path.secure().map_err(io)?;
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
            let mut pressure = AcceptPressure::default();
            while !shutdown.load(Ordering::Acquire) {
                let mut index = 0;
                while index < clients.len() {
                    if clients[index].is_finished() {
                        let _ = clients.swap_remove(index).join();
                    } else {
                        index += 1;
                    }
                }
                let was_degraded = pressure.failures > 0;
                let accepted = accept_once(
                    &mut pressure,
                    || listener.accept(),
                    |error, log| {
                        if log {
                            tracing::warn!(%error, "API accept degraded; retrying: {error}");
                        }
                        let _ = context.state.publish_component(crate::component(
                            "api",
                            "degraded",
                            error.to_string(),
                        ));
                    },
                    |delay| {
                        let end = Instant::now() + delay;
                        while !shutdown.load(Ordering::Acquire) && Instant::now() < end {
                            std::thread::park_timeout(Duration::from_millis(20));
                        }
                    },
                );
                match accepted {
                    Ok(Some((stream, _))) => {
                        let _ = context.state.publish_component(crate::component(
                            "api",
                            "ok",
                            "Unix socket listening",
                        ));
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
                    Ok(None) => {
                        if was_degraded && pressure.failures == 0 {
                            let _ = context.state.publish_component(crate::component(
                                "api",
                                "ok",
                                "Unix socket listening",
                            ));
                        }
                    }
                    Err(e) => {
                        outcome = Err(io(e));
                        shutdown.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            drop(listener);
            drop(path);
            for h in clients {
                let _ = h.join();
            }
            outcome
        })
        .map_err(io)
}
fn accept_once<T>(
    pressure: &mut AcceptPressure,
    accept: impl FnOnce() -> std::io::Result<T>,
    mut report: impl FnMut(&std::io::Error, bool),
    mut wait: impl FnMut(Duration),
) -> std::io::Result<Option<T>> {
    match accept() {
        Ok(client) => {
            pressure.failures = 0;
            Ok(Some(client))
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            pressure.failures = 0;
            wait(Duration::from_millis(20));
            Ok(None)
        }
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Ok(None),
        Err(error)
            if crate::resource_pressure(&error)
                || error.raw_os_error() == Some(rustix::io::Errno::CONNABORTED.raw_os_error()) =>
        {
            let (delay, log) = pressure.retry(Instant::now());
            report(&error, log);
            wait(delay);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
#[derive(Default)]
struct AcceptPressure {
    failures: u32,
    last_log: Option<Instant>,
}
impl AcceptPressure {
    fn retry(&mut self, now: Instant) -> (Duration, bool) {
        let delay = Duration::from_millis(50u64 << self.failures.min(4));
        self.failures = self.failures.saturating_add(1);
        let log = self
            .last_log
            .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(60));
        if log {
            self.last_log = Some(now);
        }
        (delay, log)
    }
}
#[cfg(test)]
#[path = "../../../fuzz/support/seams.rs"]
mod fuzz_seams;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn hot_dispatch_reports_startup_and_replacement_boundaries() {
        use tidemark_core::path::RawPath;
        use tidemark_live::watch::Watcher;
        let config = tidemark_core::config::Config::default();
        let state = crate::state::LiveState::new(&config, 0);
        let context = Context {
            state: state.clone(),
            scans: crate::scheduler::ScanControl::default(),
            reload: Arc::new(|| unreachable!("hot does not reload")),
        };
        let request = Request {
            v: 1,
            id: "hot-boundary".into(),
            method: "hot".into(),
            params: serde_json::json!({"mount": "/A", "by": "dir", "window": "5m", "limit": 10}),
        };
        for start in [0, 120] {
            let watcher = Arc::new(
                Watcher::from_injected_events_at(RawPath::new(b"/A".to_vec()).unwrap(), start, [])
                    .unwrap(),
            );
            state
                .publish(
                    crate::state::Publication::Watcher(("/A".into(), watcher)),
                    crate::component("watcher:/A", "ok", "test"),
                )
                .unwrap();
            let label = if start == 0 { "00:01 UTC" } else { "00:03 UTC" };
            for (at, ending) in [
                (start, "no complete window yet".to_string()),
                (
                    start + 60,
                    "ask for a window of at most 1 minutes".to_string(),
                ),
            ] {
                let response = dispatch(&request, 0, &context, at).unwrap();
                assert!(!response.ok);
                let error = response.error.unwrap();
                assert_eq!(error.code, ApiErrorCode::Unavailable);
                assert_eq!(
                    error.message,
                    format!("unavailable: watch history for /A starts at {label}; {ending}")
                );
            }
        }
    }
    #[test]
    fn writers_unavailable_names_the_process_scan_and_failure() {
        let mut config = tidemark_core::config::Config::default();
        config.procscan.enabled = true;
        let state = crate::state::LiveState::new(&config, 0);
        let context = Context {
            state: state.clone(),
            scans: crate::scheduler::ScanControl::default(),
            reload: Arc::new(|| unreachable!("writers does not reload")),
        };
        let request = Request {
            v: 1,
            id: "writers-test".into(),
            method: "writers".into(),
            params: serde_json::json!({"window": "5m"}),
        };
        let response = dispatch(&request, 0, &context, 0).unwrap();
        assert_eq!(
            response.error.unwrap().message,
            "unavailable: process scan has not completed yet"
        );
        state.publish(crate::state::Publication::ProcscanAttempt { generation: 0, report: None }, crate::component("procscan", "degraded", "process inventory over its memory budget: 34000 descriptors, 134217000 bytes retained")).unwrap();
        let response = dispatch(&request, 0, &context, 1).unwrap();
        assert_eq!(response.error.unwrap().message, "unavailable: process inventory over its memory budget: 34000 descriptors, 134217000 bytes retained");
        state
            .publish_component(crate::component("procscan", "error", "permission denied"))
            .unwrap();
        let response = dispatch(&request, 0, &context, 1).unwrap();
        assert_eq!(
            response.error.unwrap().message,
            "unavailable: process scan failed: permission denied"
        );
        config.procscan.enabled = false;
        state.replace_config(config).unwrap();
        let response = dispatch(&request, 0, &context, 1).unwrap();
        assert_eq!(
            response.error.unwrap().message,
            "unavailable: process scan not configured: disabled by config"
        );
    }
    const FRAME_SEED: &[u8] = include_bytes!("../../../fuzz/corpus/api_frame/sample");
    fn exercise_frame(bytes: &[u8]) {
        let mut input = bytes;
        let mut decoder = FrameDecoder::default();
        let mut calls = 0;
        loop {
            let before = input.len();
            let result = read_frame(&mut input, &mut decoder);
            assert!(decoder.line.len() <= LIMIT);
            assert!(decoder.line.capacity() <= LIMIT);
            calls += 1;
            assert!(calls <= bytes.len() + 1);
            match result {
                Ok(_) | Err(FrameError::InvalidJson(_)) => assert!(input.len() < before),
                Err(_) => break,
            }
        }
    }
    #[test]
    fn frame_arbitrary_bytes_and_mutated_valid_requests() {
        super::fuzz_seams::generated(exercise_frame, &[FRAME_SEED, b"{}\n", b"\xff\n"]);
    }
    #[test]
    fn frame_round_trip_chunking_and_typed_refusals() {
        let mut decoder = FrameDecoder::default();
        let mut input = FRAME_SEED;
        let request = read_frame(&mut input, &mut decoder).unwrap();
        assert_eq!(request.id, "x");
        assert_eq!(request.method, "status");
        assert_eq!(request.v, 1);
        assert_eq!(request.params, serde_json::json!({}));
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        assert_eq!(
            read_frame(&mut encoded.as_slice(), &mut decoder)
                .unwrap()
                .id,
            "x"
        );
        for byte in &FRAME_SEED[..FRAME_SEED.len() - 1] {
            assert!(matches!(
                read_frame(&mut std::slice::from_ref(byte), &mut decoder),
                Err(FrameError::Incomplete)
            ));
        }
        assert_eq!(
            read_frame(&mut b"\n".as_slice(), &mut decoder).unwrap().id,
            "x"
        );
        assert!(matches!(
            read_frame(&mut b"".as_slice(), &mut decoder),
            Err(FrameError::EndOfInput)
        ));
        let mut two = b"bad\n".to_vec();
        two.extend_from_slice(FRAME_SEED);
        let mut input = two.as_slice();
        assert!(matches!(
            read_frame(&mut input, &mut decoder),
            Err(FrameError::InvalidJson(_))
        ));
        assert_eq!(input, FRAME_SEED);
        assert_eq!(read_frame(&mut input, &mut decoder).unwrap().id, "x");
    }
    #[test]
    fn frame_limit_includes_newline_and_read_failures_do_not_spin() {
        let mut exact = vec![b' '; LIMIT - FRAME_SEED.len()];
        exact.extend_from_slice(FRAME_SEED);
        assert_eq!(exact.len(), LIMIT);
        assert!(read_frame(&mut exact.as_slice(), &mut FrameDecoder::default()).is_ok());
        exact.insert(0, b' ');
        let mut input = exact.as_slice();
        let mut decoder = FrameDecoder::default();
        assert!(matches!(
            read_frame(&mut input, &mut decoder),
            Err(FrameError::TooLarge)
        ));
        assert!(decoder.line.capacity() <= LIMIT);
        struct Interrupted;
        impl Read for Interrupted {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::Interrupted.into())
            }
        }
        assert!(matches!(
            read_frame(&mut Interrupted, &mut FrameDecoder::default()),
            Err(FrameError::Io(_))
        ));
    }
    #[test]
    fn frame_large_inputs_and_scaling() {
        // Whitespace has no newline and exercises full-buffer framing.
        super::fuzz_seams::scaling(b" ", exercise_frame);
        for byte in [0, 255, b'9'] {
            super::fuzz_seams::timed(&vec![byte; LIMIT + 1], exercise_frame);
        }
        let mut deep = b"{\"v\":1,\"id\":\"x\",\"method\":\"status\",\"params\":".to_vec();
        deep.extend(std::iter::repeat_n(b'[', 512));
        deep.extend(std::iter::repeat_n(b']', 512));
        deep.extend_from_slice(b"}\n");
        assert!(matches!(
            read_frame(&mut deep.as_slice(), &mut FrameDecoder::default()),
            Err(FrameError::InvalidJson(_))
        ));
        super::fuzz_seams::timed(&deep, exercise_frame);
    }

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
    fn twenty_resource_accept_failures_back_off_then_recover() {
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut pressure = AcceptPressure::default();
        let mut warnings = 0;
        let mut degraded = 0;
        for n in 0..21u32 {
            let result = accept_once(
                &mut pressure,
                || {
                    if n < 20 {
                        Err(std::io::Error::from_raw_os_error(24))
                    } else {
                        Ok(42)
                    }
                },
                |_, log| {
                    degraded += 1;
                    warnings += usize::from(log);
                },
                |delay| assert_eq!(delay.as_millis(), 50u128 << n.min(4)),
            )
            .unwrap();
            assert_eq!(result, if n < 20 { None } else { Some(42) });
            assert!(!shutdown.load(Ordering::Acquire));
        }
        assert_eq!((warnings, degraded, pressure.failures), (1, 20, 0));
        assert_eq!(
            pressure.retry(Instant::now() + Duration::from_secs(61)),
            (Duration::from_millis(50), true)
        );
        assert!(accept_once::<()>(
            &mut pressure,
            || Err(std::io::Error::from_raw_os_error(9)),
            |_, _| panic!("fatal error"),
            |_| panic!("fatal error")
        )
        .is_err());
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
            // Allow the client to be scheduled under workspace load before the
            // idle deadline closes the pair. The three-second reader limit
            // still fails if the callback does not observe its real deadline.
            let (mut stream, stop, handle) = pair(reload, Duration::from_secs(1));
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
        let admitted = Cancellation::new(stop.clone(), TIMEOUT);
        admitted.begin_scan_admission().unwrap();
        admitted.cancel();
        assert!(admitted.check().is_err());
        let refused_scan = Cancellation::new(stop.clone(), TIMEOUT);
        refused_scan.cancel();
        assert!(refused_scan.begin_scan_admission().is_err());
        let expired = Cancellation::new(stop.clone(), Duration::ZERO);
        assert!(expired.begin_commit().is_err());
        let stopping = Cancellation::new(stop.clone(), TIMEOUT);
        stop.store(true, Ordering::Release);
        assert!(stopping.begin_commit().is_err());
    }
    #[test]
    fn socket_lock_excludes_same_socket_reuses_crash_file_and_preserves_replacement() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("api.sock");
        let lock = dir.path().join("api.sock.lock");
        std::fs::write(&lock, "crash leftover").unwrap();
        let guard = SocketPath::open(&socket).unwrap();
        assert_eq!(
            std::fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(SocketPath::open(&socket).is_err());
        crate::metrics::write_textfile(&dir.path().join("metrics.prom"), "exported").unwrap();
        let other = SocketPath::open(&dir.path().join("other.sock")).unwrap();
        drop(other);
        std::fs::rename(&lock, dir.path().join("old.lock")).unwrap();
        std::fs::write(&lock, "replacement").unwrap();
        drop(guard);
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "replacement");
        let guard = SocketPath::open(&socket).unwrap();
        drop(guard);
        assert!(!lock.exists());
        symlink(dir.path().join("target"), &lock).unwrap();
        assert!(SocketPath::open(&socket).is_err());
        std::fs::remove_file(&lock).unwrap();
        std::fs::write(&lock, "linked").unwrap();
        std::fs::hard_link(&lock, dir.path().join("alias")).unwrap();
        assert!(SocketPath::open(&socket).is_err());
    }
    #[test]
    fn socket_parent_trust_and_no_follow_checks_without_listening() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        match SocketPath::open(&private.join("api.sock")) {
            Ok(_) => {}
            Err(e) => panic!("trusted parent refused: {e}"),
        }
        let link = dir.path().join("link");
        symlink(&private, &link).unwrap();
        assert!(SocketPath::open(&link.join("api.sock")).is_err());
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(SocketPath::open(&private.join("api.sock")).is_err());
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        symlink(private.join("target"), private.join("api.sock")).unwrap();
        let mut socket = SocketPath::open(&private.join("api.sock")).unwrap();
        assert!(socket.prepare(Duration::from_millis(100)).is_err());
        assert!(socket.secure().is_err());
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
