//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Command arguments.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Operation.
    #[command(subcommand)]
    pub command: Command,
}
/// Available operations.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// List operation.
    List {
        /// Root.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
    /// Show operation.
    Show {
        /// Id.
        #[arg(value_name = "ID")]
        id: String,
        /// Root.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
    /// Prune operation.
    Prune {
        /// Root.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
        /// Dry run.
        #[arg(long)]
        dry_run: bool,
    },
    /// RebuildCatalog operation.
    RebuildCatalog {
        /// Root.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
}
/// Execute the operation.
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
}
