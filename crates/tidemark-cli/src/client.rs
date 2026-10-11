//! Owned by: W4-B
//! Bounded synchronous JSON-lines client for the frozen daemon API.

use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};
use tidemark_core::api::{ApiError, Request, Response};

/// Maximum request or response size, including its newline (1 MiB).
const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// A persistent connection; calls are sequential and use monotonically increasing ids.
pub struct Client {
    stream: BufReader<UnixStream>,
    timeout: Duration,
    sequence: u64,
    poisoned: bool,
}
impl Client {
    /// Connect to the daemon and configure per-call read and write deadlines.
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Client, ClientError> {
        if timeout.is_zero() {
            return Err(ClientError::Timeout);
        }
        let stream = connect_deadline(socket, timeout)?;
        stream.set_read_timeout(Some(timeout)).map_err(map_io)?;
        stream.set_write_timeout(Some(timeout)).map_err(map_io)?;
        Ok(Self {
            stream: BufReader::with_capacity(8192, stream),
            timeout,
            sequence: 0,
            poisoned: false,
        })
    }
    /// Send one request and validate the matching response before decoding its result.
    pub fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<R, ClientError> {
        if self.poisoned {
            return Err(ClientError::Protocol(
                "connection unusable after failed call",
            ));
        }
        self.poisoned = true;
        let result = self.exchange(method, params);
        if result.is_ok() || matches!(result, Err(ClientError::Remote(_))) {
            self.poisoned = false;
        }
        result
    }
    fn exchange<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<R, ClientError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(ClientError::Protocol("request id exhausted"))?;
        let id = format!("c{}", self.sequence);
        let request = Request {
            v: 1,
            id: id.clone(),
            method: method.into(),
            params: serde_json::to_value(params)?,
        };
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(ClientError::Protocol("request exceeds 1 MiB"));
        }
        let started = Instant::now();
        let mut remaining = bytes.as_slice();
        while !remaining.is_empty() {
            self.stream
                .get_mut()
                .set_write_timeout(Some(deadline(self.timeout, started)?))
                .map_err(map_io)?;
            let count = self.stream.get_mut().write(remaining).map_err(map_io)?;
            if count == 0 {
                return Err(ClientError::Unavailable);
            }
            remaining = &remaining[count..];
        }
        bytes.clear();
        // Buffer transport reads, retaining any coalesced next frame on this connection.
        loop {
            self.stream
                .get_ref()
                .set_read_timeout(Some(deadline(self.timeout, started)?))
                .map_err(map_io)?;
            let available = self.stream.fill_buf().map_err(map_io)?;
            if available.is_empty() {
                return Err(ClientError::Unavailable);
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let count = newline.unwrap_or(available.len());
            if count > MAX_FRAME_BYTES - 1 - bytes.len() {
                return Err(ClientError::Protocol("response exceeds 1 MiB"));
            }
            bytes.extend_from_slice(&available[..count]);
            self.stream.consume(count + usize::from(newline.is_some()));
            if newline.is_some() {
                break;
            }
        }
        let response: Response = serde_json::from_slice(&bytes)?;
        if response.v != 1 || response.id != id {
            return Err(ClientError::Protocol("response version or id mismatch"));
        }
        match (response.ok, response.result, response.error) {
            (true, Some(value), None) => Ok(serde_json::from_value(value)?),
            (false, None, Some(error)) => Err(ClientError::Remote(error)),
            _ => Err(ClientError::Protocol("invalid response envelope")),
        }
    }
}
// std exposes no nonblocking Unix connect constructor. Bound both caller waiting and
// outstanding kernel connects without adding a dependency feature outside this task.
fn connect_deadline(socket: &Path, timeout: Duration) -> Result<UnixStream, ClientError> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    static CONNECTING: AtomicUsize = AtomicUsize::new(0);
    const MAX_CONNECTING: usize = 16;
    CONNECTING
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_CONNECTING).then_some(count + 1)
        })
        .map_err(|_| ClientError::Timeout)?;
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            CONNECTING.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let permit = Permit;
    let path = socket.to_path_buf();
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("tidemark-api-connect".into())
        .spawn(move || {
            let _permit = permit;
            let result = UnixStream::connect(path);
            // A timed-out caller has dropped the receiver; dropping the result closes its fd.
            let _ = sender.send(result);
        })
        .map_err(ClientError::Io)?;
    receiver
        .recv_timeout(timeout)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => ClientError::Timeout,
            mpsc::RecvTimeoutError::Disconnected => ClientError::Unavailable,
        })?
        .map_err(map_io)
}
fn deadline(timeout: Duration, started: Instant) -> Result<Duration, ClientError> {
    timeout
        .checked_sub(started.elapsed())
        .filter(|v| !v.is_zero())
        .ok_or(ClientError::Timeout)
}
fn map_io(error: std::io::Error) -> ClientError {
    use std::io::ErrorKind::*;
    match error.kind() {
        NotFound | ConnectionRefused | ConnectionReset | BrokenPipe | UnexpectedEof => {
            ClientError::Unavailable
        }
        TimedOut | WouldBlock => ClientError::Timeout,
        _ => ClientError::Io(error),
    }
}
/// Client failures; unavailable endpoints retain CLI exit code 4.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Compatibility variant retained from preparation.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// No daemon is listening, or it disconnected.
    #[error("daemon unavailable")]
    Unavailable,
    /// The call deadline expired.
    #[error("daemon call timed out")]
    Timeout,
    /// Transport failure.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Invalid JSON or incompatible result type.
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    /// Invalid framing or envelope.
    #[error("API protocol error: {0}")]
    Protocol(&'static str),
    /// A typed daemon error response.
    #[error("daemon error: {0:?}")]
    Remote(ApiError),
}
impl ClientError {
    /// Stable CLI exit status.
    pub fn exit_code(&self) -> u8 {
        if matches!(self, Self::Unavailable) {
            4
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        os::unix::net::UnixListener,
        thread,
    };
    fn pair(timeout: Duration) -> (Client, UnixStream) {
        let (stream, server) = UnixStream::pair().unwrap();
        (
            Client {
                stream: BufReader::with_capacity(8192, stream),
                timeout,
                sequence: 0,
                poisoned: false,
            },
            server,
        )
    }
    fn respond(mut stream: UnixStream, response: &[u8]) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        stream.write_all(response).unwrap();
    }
    #[test]
    fn framing_and_repeated_calls() {
        let (mut client, mut stream) = pair(Duration::from_secs(2));
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for index in 1..=2 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let req: Request = serde_json::from_str(&line).unwrap();
                assert_eq!(req.id, format!("c{index}"));
                assert_eq!(req.method, "status");
                assert_eq!(req.params, serde_json::json!({}));
                let response =
                    format!("{{\"v\":1,\"id\":\"c{index}\",\"ok\":true,\"result\":{index}}}\n");
                for byte in response.bytes() {
                    stream.write_all(&[byte]).unwrap();
                }
            }
        });
        for n in 1..=2 {
            assert_eq!(
                client
                    .call::<_, u32>("status", &serde_json::json!({}))
                    .unwrap(),
                n
            );
        }
        server.join().unwrap();
    }
    #[test]
    fn absent_and_refused_errors_map_to_exit_four() {
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::ConnectionRefused,
        ] {
            assert_eq!(map_io(std::io::Error::from(kind)).exit_code(), 4);
        }
    }
    #[test]
    fn unix_listener_connect_and_daemon_absence() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("api.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.raw_os_error() == Some(1) => {
                eprintln!("UnixListener coverage unavailable: sandbox rejects AF_UNIX socket creation (EPERM); socketpair transport tests still run");
                return;
            }
            Err(error) => panic!("cannot create listener: {error}"),
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            respond(
                stream,
                b"{\"v\":1,\"id\":\"c1\",\"ok\":true,\"result\":7}\n",
            );
        });
        let mut client = Client::connect(&socket, Duration::from_secs(1)).unwrap();
        assert_eq!(
            client
                .call::<_, u32>("status", &serde_json::json!({}))
                .unwrap(),
            7
        );
        server.join().unwrap();
        assert_eq!(
            Client::connect(&socket, Duration::from_secs(1))
                .err()
                .unwrap()
                .exit_code(),
            4
        );
        std::fs::remove_file(&socket).unwrap();
        assert_eq!(
            Client::connect(&socket, Duration::from_secs(1))
                .err()
                .unwrap()
                .exit_code(),
            4
        );
    }
    #[test]
    fn deadline_and_poisoned_connection() {
        // Keep the peer open until both assertions finish. A sleeping peer that
        // closes can race a delayed test thread and turn timeout into EOF.
        let (mut client, _peer) = pair(Duration::from_millis(20));
        let error = client.call::<_, u32>("status", &()).unwrap_err();
        assert!(matches!(error, ClientError::Timeout), "{error:?}");
        assert!(matches!(
            client.call::<_, u32>("status", &()),
            Err(ClientError::Protocol(_))
        ));
    }
    #[test]
    fn mismatched_id_version_and_invalid_envelope_rejected() {
        for response in [
            b"{\"v\":1,\"id\":\"wrong\",\"ok\":true,\"result\":0}\n".as_slice(),
            b"{\"v\":2,\"id\":\"c1\",\"ok\":true,\"result\":0}\n".as_slice(),
            b"{\"v\":1,\"id\":\"c1\",\"ok\":true}\n".as_slice(),
        ] {
            let (mut client, stream) = pair(Duration::from_secs(2));
            let response = response.to_vec();
            let server = thread::spawn(move || respond(stream, &response));
            assert!(matches!(
                client.call::<_, u32>("status", &()),
                Err(ClientError::Protocol(_))
            ));
            server.join().unwrap();
        }
    }
    #[test]
    fn remote_error_does_not_poison_connection() {
        let (mut client, mut stream) = pair(Duration::from_secs(2));
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"{\"v\":1,\"id\":\"c1\",\"ok\":false,\"error\":{\"code\":\"busy\",\"message\":\"busy\"}}\n").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            stream
                .write_all(b"{\"v\":1,\"id\":\"c2\",\"ok\":true,\"result\":2}\n")
                .unwrap();
        });
        assert!(matches!(
            client.call::<_, u32>("scan.start", &()),
            Err(ClientError::Remote(_))
        ));
        assert_eq!(client.call::<_, u32>("status", &()).unwrap(), 2);
        server.join().unwrap();
    }
    #[test]
    fn truncated_and_malformed_frames_rejected() {
        for response in [b"{broken}\n".as_slice(), b"{\"v\":1}".as_slice()] {
            let (mut client, stream) = pair(Duration::from_secs(2));
            let response = response.to_vec();
            let server = thread::spawn(move || respond(stream, &response));
            assert!(client.call::<_, u32>("status", &()).is_err());
            server.join().unwrap();
        }
    }
    #[test]
    fn oversized_frames_are_bounded() {
        let (mut client, stream) = pair(Duration::from_secs(5));
        let response = vec![b'x'; MAX_FRAME_BYTES];
        let server = thread::spawn(move || {
            let mut stream = stream;
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let _ = stream.write_all(&response);
        });
        assert!(matches!(
            client.call::<_, u32>("status", &()),
            Err(ClientError::Protocol(_))
        ));
        drop(client);
        server.join().unwrap();
        let (mut client, _stream) = pair(Duration::from_secs(2));
        assert!(matches!(
            client.call::<_, u32>("status", &"x".repeat(MAX_FRAME_BYTES)),
            Err(ClientError::Protocol(_))
        ));
    }
    #[test]
    fn maximum_frame_includes_its_newline() {
        let header = b"{\"v\":1,\"id\":\"c1\",\"ok\":true,\"result\":\"";
        let tail = b"\"}\n";
        let length = MAX_FRAME_BYTES - header.len() - tail.len();
        let mut response = header.to_vec();
        response.extend(std::iter::repeat_n(b'x', length));
        response.extend_from_slice(tail);
        assert_eq!(response.len(), MAX_FRAME_BYTES);
        let (mut client, stream) = pair(Duration::from_secs(5));
        let server = thread::spawn(move || respond(stream, &response));
        let result: String = client.call("status", &serde_json::json!({})).unwrap();
        assert_eq!(result.len(), length);
        server.join().unwrap();
    }
}
