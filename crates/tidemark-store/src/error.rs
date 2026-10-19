//! Owned by: W1-E
//! Typed failures for snapshot and time-series storage.

/// A storage operation could not complete.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Snapshot publication was cancelled before rename.
    #[error("snapshot publication cancelled")]
    Cancelled,
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// SQLite operation failed.
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    /// Metadata JSON was invalid.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A stored path failed validation.
    #[error(transparent)]
    Core(#[from] tidemark_core::error::CoreError),
    /// Integer cannot be represented by the storage contract.
    #[error("integer outside storage range: {0}")]
    Integer(String),
    /// Invalid schema, metadata, or query.
    #[error("invalid storage data: {0}")]
    Invalid(String),
    /// State space policy refused additional rows.
    #[error("state space exhausted: {0}")]
    Space(String),
    /// An export destination already exists and was left unchanged.
    ///
    /// Only this variant is an output-path precondition. Other `AlreadyExists`
    /// I/O errors are not.
    #[error("output path already exists: {0}")]
    OutputExists(String),
}
pub(crate) fn sql_u64(n: u64) -> Result<i64, StoreError> {
    i64::try_from(n).map_err(|_| StoreError::Integer(n.to_string()))
}
pub(crate) fn sql_limit(n: usize) -> Result<i64, StoreError> {
    i64::try_from(n).map_err(|_| StoreError::Integer(n.to_string()))
}

pub(crate) fn read_u64(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let n: i64 = r.get(index)?;
    u64::try_from(n).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, n))
}
pub(crate) fn read_optional_u64(
    r: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<u64>> {
    r.get::<_, Option<i64>>(index)?
        .map(|n| u64::try_from(n).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, n)))
        .transpose()
}

// Bound APIs whose frozen signatures have no limit, and guard caller-supplied limits.
pub(crate) const MAX_QUERY_ROWS: usize = 1_000_000;
pub(crate) fn check_result_bound<T>(rows: &[T]) -> Result<(), StoreError> {
    if rows.len() > MAX_QUERY_ROWS {
        return Err(StoreError::Invalid(
            "query exceeds 1,000,000-row result cap; narrow the query".into(),
        ));
    }
    Ok(())
}
