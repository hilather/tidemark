//! Owned by: W4-B
//! Compiling cross-task interfaces; runtime behavior belongs to W4-B.

use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, time::Duration};
/// Cross-task contract reserved for the owning task.
pub struct Client;
impl Client {
    /// Cross-task contract reserved for the owning task.
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Client, ClientError> {
        let _ = socket;
        let _ = timeout;
        Err(ClientError::NotYetImplemented)
    }
    /// Cross-task contract reserved for the owning task.
    pub fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<R, ClientError> {
        let _ = method;
        let _ = params;
        Err(ClientError::NotYetImplemented)
    }
}

#[derive(Debug, thiserror::Error)]
/// Cross-task contract reserved for the owning task.
pub enum ClientError {
    #[doc = "Contract variant."]
    #[error("NotYetImplemented")]
    NotYetImplemented,
    #[doc = "Contract variant."]
    #[error("daemon unavailable")]
    Unavailable,
}
