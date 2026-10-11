//! Owned by: W3-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path.
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Boundary.
    #[arg(long, value_name = "GLOB", num_args = 1.., conflicts_with = "boundary_depth")]
    pub boundary: Vec<String>,
    /// Boundary depth.
    #[arg(long, value_name = "N")]
    pub boundary_depth: Option<usize>,
    /// Incomplete.
    #[arg(long)]
    pub incomplete: bool,
    /// Top.
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
    /// Show paths.
    #[arg(long)]
    pub show_paths: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-B)"))
}
