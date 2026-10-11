//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path.
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Limit.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
    /// Min size.
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<tidemark_core::units::ByteSize>,
    /// Older than.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration)]
    pub older_than: Option<std::time::Duration>,
    /// Newer than.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration)]
    pub newer_than: Option<std::time::Duration>,
    /// Snapshot.
    #[arg(long, value_name = "ID")]
    pub snapshot: Option<String>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
}
