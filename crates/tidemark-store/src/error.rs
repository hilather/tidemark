//! Owned by: W1-E
//! Snapshot storage error types.

/// Stub error reserved for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
