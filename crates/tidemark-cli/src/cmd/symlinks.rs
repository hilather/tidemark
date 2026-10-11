//! Owned by: W3-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path.
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Check targets.
    #[arg(long)]
    pub check_targets: bool,
    /// Dangling.
    #[arg(long)]
    pub dangling: bool,
    /// Escaping.
    #[arg(long)]
    pub escaping: bool,
    /// Limit.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-B)"))
}
