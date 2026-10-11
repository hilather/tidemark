//! Owned by: W0-B1
//! Json contracts and helpers.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
/// Versioned envelope for command JSON output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonDoc<T> {
    /// Command schema, e.g. `tidemark.now.v1`; output may borrow a constant, while input owns its schema.
    pub schema: Cow<'static, str>,
    /// Command output.
    pub data: T,
}
