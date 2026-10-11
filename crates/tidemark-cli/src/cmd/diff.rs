//! Owned by: W3-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Compare with a snapshot this duration ago.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration, conflicts_with = "from")]
    pub since: Option<std::time::Duration>,
    /// Starting snapshot ID.
    #[arg(long, value_name = "ID")]
    pub from: Option<String>,
    /// Ending snapshot ID.
    #[arg(long, value_name = "ID", requires = "from")]
    pub to: Option<String>,
    /// Directory depth to display.
    #[arg(long, value_name = "N", default_value_t = 3)]
    pub depth: usize,
    /// Metric to compare between snapshots.
    #[arg(long, value_parser = ["disk", "reclaimable", "files"])]
    pub metric: Option<String>,
    /// Explain the sources of changes.
    #[arg(long)]
    pub explain: bool,
    /// Minimum fraction of change to explain.
    #[arg(long, value_name = "F", default_value_t = 0.5)]
    pub explain_ratio: f64,
    /// Minimum size of change to explain.
    #[arg(long, value_name = "SIZE", default_value = "1GiB")]
    pub explain_min: tidemark_core::units::ByteSize,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-A)"))
}
