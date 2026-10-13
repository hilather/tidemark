//! Owned by: W7-C3-fuzz
//! Exercise the public client decoder using a disposable local socket.
// Fixture setup errors must fail immediately; EPERM is a documented sandbox exclusion.
#![allow(clippy::unwrap_used)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    thread,
    time::Duration,
};
/// Return false only when the sandbox prohibits Unix listener creation.
pub fn exercise(bytes: &[u8]) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("api.sock");
    let listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(error) if error.raw_os_error() == Some(1) => return false,
        Err(error) => panic!("Unix fixture listener: {error}"),
    };
    let mut client =
        tidemark_cli::client::Client::connect(&socket, Duration::from_secs(1)).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let bytes = bytes.to_vec();
    let server = thread::spawn(move || {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = Vec::new();
        reader.read_until(b'\n', &mut request).unwrap();
        let _ = stream.write_all(&bytes);
    });
    let _: Result<serde_json::Value, tidemark_cli::client::ClientError> =
        client.call("status", &());
    drop(client);
    server.join().unwrap();
    true
}
