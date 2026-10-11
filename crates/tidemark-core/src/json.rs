//! Owned by: W0-B1
//! Json contracts and helpers.

use serde::{Deserialize, Serialize};
/// Versioned envelope for command JSON output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonDoc<T> {
    /// Command schema, e.g. `tidemark.now.v1`.
    pub schema: &'static str,
    /// Command output.
    pub data: T,
}
