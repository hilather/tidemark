//! Owned by: W3-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Boundary path globs for hard-link analysis.
    #[arg(long, value_name = "GLOB", num_args = 1.., conflicts_with = "boundary_depth")]
    pub boundary: Vec<String>,
    /// Directory depth defining hard-link boundaries.
    #[arg(long, value_name = "N")]
    pub boundary_depth: Option<usize>,
    /// Show incomplete hard-link sets.
    #[arg(long)]
    pub incomplete: bool,
    /// Maximum number of hard-link sets.
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
    /// Resolve hard-link names live and show paths.
    #[arg(long)]
    pub show_paths: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-B)"))
}
