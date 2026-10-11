//! Owned by: W3-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Stat targets to detect dangling, looping or other-filesystem links.
    #[arg(long)]
    pub check_targets: bool,
    /// List only dangling symlinks.
    #[arg(long)]
    pub dangling: bool,
    /// List only links resolving outside PATH.
    #[arg(long)]
    pub escaping: bool,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-B)"))
}
