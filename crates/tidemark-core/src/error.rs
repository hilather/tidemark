//! Owned by: W0-B1
//! Error contracts and helpers.

use thiserror::Error;
/// Errors parsing or loading core contracts.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Invalid raw path or name.
    #[error("invalid path: {0}")]
    /// Path variant.
    Path(String),
    /// Invalid size, rate or time.
    #[error("invalid value: {0}")]
    /// Parse variant.
    Parse(String),
    /// Invalid TOML configuration, including unknown keys.
    #[error("invalid configuration: {0}")]
    /// Config variant.
    Config(#[from] toml::de::Error),
    /// Failure reading the configuration file.
    #[error("cannot read configuration: {0}")]
    /// Io variant.
    Io(#[from] std::io::Error),
}
