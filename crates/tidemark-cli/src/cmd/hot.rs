//! Owned by: W6-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Time window for activity.
    #[arg(long, value_parser = ["1m", "5m", "15m", "60m"])]
    pub window: Option<String>,
    /// Group activity by directory, unit or command.
    #[arg(long, value_parser = ["dir", "unit", "comm"])]
    pub by: Option<String>,
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W6-A)"))
}
