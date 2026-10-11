//! Owned by: W3-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path.
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Min entries.
    #[arg(long, value_name = "N", default_value_t = 10000)]
    pub min_entries: usize,
    /// Limit.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-C)"))
}
